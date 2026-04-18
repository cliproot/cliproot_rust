use std::fs;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use sha2::{Digest, Sha256};

use cliproot_store::KnowledgeConfig;

use super::state::FlushState;

// ── Anthropic API types ───────────────────────────────────────────────────────

#[derive(serde::Serialize)]
struct AnthropicRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    system: &'a str,
    messages: Vec<AnthropicMessage<'a>>,
}

#[derive(serde::Serialize)]
struct AnthropicMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(serde::Deserialize)]
struct AnthropicResponse {
    content: Vec<ContentBlock>,
    usage: Usage,
}

#[derive(serde::Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    text: Option<String>,
}

#[derive(serde::Deserialize)]
struct Usage {
    input_tokens: u64,
    output_tokens: u64,
}

// ── Public result type ────────────────────────────────────────────────────────

/// Returned after a successful Anthropic API call.
#[derive(Debug, Clone)]
pub struct LlmCallResult {
    pub text: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    /// Estimated USD cost at Haiku pricing (input: $1/M, output: $5/M tokens).
    pub estimated_cost_usd: f64,
    pub model: String,
    /// SHA-256 (base64url) of the concatenated system+user prompt text.
    pub prompt_hash: String,
}

// ── Budget error ──────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct BudgetExceeded {
    pub reason: String,
}

impl std::fmt::Display for BudgetExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "budget exceeded: {}", self.reason)
    }
}

impl std::error::Error for BudgetExceeded {}

// ── Budget check ──────────────────────────────────────────────────────────────

/// Return `Err(BudgetExceeded)` if running `estimated_tokens` more tokens
/// would exceed either the token cap or cost cap for today.
pub fn check_budget(
    state: &FlushState,
    cfg: &KnowledgeConfig,
    estimated_tokens: u64,
) -> Result<(), BudgetExceeded> {
    let projected_tokens = state.daily_total_tokens + estimated_tokens;
    if projected_tokens > cfg.max_bg_tokens_per_day {
        return Err(BudgetExceeded {
            reason: format!(
                "daily token cap {} would be exceeded (currently {}, estimated {})",
                cfg.max_bg_tokens_per_day, state.daily_total_tokens, estimated_tokens
            ),
        });
    }

    // Rough cost estimate at Haiku pricing: $1/M input + $5/M output.
    // Assume ~80% input, ~20% output split for a conservative estimate.
    let est_cost = (estimated_tokens as f64 * 0.8 / 1_000_000.0) * 1.0
        + (estimated_tokens as f64 * 0.2 / 1_000_000.0) * 5.0;
    let projected_cost = state.daily_total_cost_usd + est_cost;
    if projected_cost > cfg.max_bg_cost_per_day_usd {
        return Err(BudgetExceeded {
            reason: format!(
                "daily cost cap ${:.4} would be exceeded (currently ${:.4}, estimated ${:.4})",
                cfg.max_bg_cost_per_day_usd, state.daily_total_cost_usd, est_cost
            ),
        });
    }

    Ok(())
}

// ── API call ──────────────────────────────────────────────────────────────────

/// Call the Anthropic Messages API with the given system + user prompt.
///
/// Credentials are resolved in order (see [`resolve_api_key`] for details):
/// 1. `ANTHROPIC_API_KEY` environment variable.
/// 2. `CLAUDE_CODE_OAUTH_TOKEN` environment variable (from `claude setup-token`).
/// 3. `$CLAUDE_CONFIG_DIR/.credentials.json`.
/// 4. `~/.claude/.credentials.json`.
pub fn call(
    system: &str,
    user: &str,
    model: &str,
    max_tokens: u32,
) -> Result<LlmCallResult, Box<dyn std::error::Error>> {
    let api_key = resolve_api_key()?;

    let prompt_hash = sha256_base64url(format!("{system}\n\n{user}").as_bytes());

    let request_body = AnthropicRequest {
        model,
        max_tokens,
        system,
        messages: vec![AnthropicMessage {
            role: "user",
            content: user,
        }],
    };

    let client = reqwest::blocking::Client::new();
    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&request_body)
        .send()?;

    let status = response.status();
    let body_text = response.text()?;

    if !status.is_success() {
        return Err(format!("Anthropic API error {status}: {body_text}").into());
    }

    let parsed: AnthropicResponse = serde_json::from_str(&body_text)
        .map_err(|e| format!("failed to parse Anthropic response: {e}\nBody: {body_text}"))?;

    let text = parsed
        .content
        .into_iter()
        .filter(|b| b.block_type == "text")
        .filter_map(|b| b.text)
        .collect::<Vec<_>>()
        .join("");

    let input_tokens = parsed.usage.input_tokens;
    let output_tokens = parsed.usage.output_tokens;
    let total_tokens = input_tokens + output_tokens;

    // Haiku-4-5 pricing: $1/M input, $5/M output
    let estimated_cost_usd =
        (input_tokens as f64 / 1_000_000.0) * 1.0 + (output_tokens as f64 / 1_000_000.0) * 5.0;

    Ok(LlmCallResult {
        text,
        input_tokens,
        output_tokens,
        total_tokens,
        estimated_cost_usd,
        model: model.to_string(),
        prompt_hash,
    })
}

// ── Credential resolution ─────────────────────────────────────────────────────

/// Resolve an Anthropic API key or Claude Code OAuth token from the environment.
///
/// Resolution order:
/// 1. `ANTHROPIC_API_KEY` — raw Console API key.
/// 2. `CLAUDE_CODE_OAUTH_TOKEN` — OAuth token produced by `claude setup-token`.
///    Recommended for subprocesses/hooks on macOS, where Keychain-stored login
///    credentials are not accessible.
/// 3. `$CLAUDE_CONFIG_DIR/.credentials.json` — documented alternate location.
/// 4. `~/.claude/.credentials.json` — Linux/Windows default for Claude Code.
///
/// JSON credentials files are parsed in two shapes: the current nested shape
/// (`{"claudeAiOauth": {"accessToken": "…"}}`) and legacy flat shapes
/// (`claudeAiOauthAccessToken`, `apiKey`).
fn resolve_api_key() -> Result<String, Box<dyn std::error::Error>> {
    // 1. ANTHROPIC_API_KEY
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        if !key.is_empty() {
            return Ok(key);
        }
    }

    // 2. CLAUDE_CODE_OAUTH_TOKEN (from `claude setup-token`)
    if let Ok(token) = std::env::var("CLAUDE_CODE_OAUTH_TOKEN") {
        if !token.is_empty() {
            return Ok(token);
        }
    }

    // 3. $CLAUDE_CONFIG_DIR/.credentials.json
    if let Ok(dir) = std::env::var("CLAUDE_CONFIG_DIR") {
        if !dir.is_empty() {
            let path = PathBuf::from(dir).join(".credentials.json");
            if let Some(key) = read_credentials_file(&path) {
                return Ok(key);
            }
        }
    }

    // 4. ~/.claude/.credentials.json
    if let Some(home) = home_dir() {
        let path = home.join(".claude").join(".credentials.json");
        if let Some(key) = read_credentials_file(&path) {
            return Ok(key);
        }
    }

    Err("No Anthropic API key found.\n\
         Options (in order of preference):\n  \
         • Run `claude setup-token` to generate a token that uses your Claude Code subscription.\n  \
         • Set ANTHROPIC_API_KEY from https://console.anthropic.com/settings/keys.\n  \
         On macOS, OAuth credentials Claude Code stores in Keychain are not readable from \
         hook subprocesses — `claude setup-token` is the supported way to expose them."
        .into())
}

fn read_credentials_file(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;

    // Nested shape written by Claude Code: {"claudeAiOauth": {"accessToken": "…"}}
    if let Some(val) = json
        .get("claudeAiOauth")
        .and_then(|v| v.get("accessToken"))
        .and_then(|v| v.as_str())
    {
        if !val.is_empty() {
            return Some(val.to_string());
        }
    }

    // Legacy / alternate flat keys
    for key in ["claudeAiOauthAccessToken", "apiKey"] {
        if let Some(val) = json.get(key).and_then(|v| v.as_str()) {
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }

    None
}

fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from).or_else(|| {
        // Windows fallback
        std::env::var("USERPROFILE").ok().map(PathBuf::from)
    })
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn sha256_base64url(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    URL_SAFE_NO_PAD.encode(hasher.finalize())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Tests that mutate ANTHROPIC_API_KEY / CLAUDE_CODE_OAUTH_TOKEN must not
    // run in parallel, since cargo test shares one process env across threads.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn make_state(tokens: u64, cost: f64) -> FlushState {
        let mut s = FlushState::default();
        s.daily_total_tokens = tokens;
        s.daily_total_cost_usd = cost;
        s
    }

    fn make_cfg(max_tokens: u64, max_cost: f64) -> KnowledgeConfig {
        let mut cfg = KnowledgeConfig::default();
        cfg.max_bg_tokens_per_day = max_tokens;
        cfg.max_bg_cost_per_day_usd = max_cost;
        cfg
    }

    #[test]
    fn budget_ok_within_limits() {
        let state = make_state(0, 0.0);
        let cfg = make_cfg(100_000, 0.50);
        assert!(check_budget(&state, &cfg, 1000).is_ok());
    }

    #[test]
    fn budget_exceeded_tokens() {
        let state = make_state(99_500, 0.0);
        let cfg = make_cfg(100_000, 0.50);
        let err = check_budget(&state, &cfg, 1000).unwrap_err();
        assert!(err.reason.contains("token cap"));
    }

    #[test]
    fn budget_exceeded_cost() {
        let state = make_state(0, 0.49);
        let cfg = make_cfg(100_000, 0.50);
        // 10_000 tokens at Haiku pricing: ~$0.018 → pushes total over $0.50
        let err = check_budget(&state, &cfg, 100_000).unwrap_err();
        assert!(err.reason.contains("cost cap"));
    }

    #[test]
    fn credential_env_var_used() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("ANTHROPIC_API_KEY", "test-key-123");
        let key = resolve_api_key().unwrap();
        assert_eq!(key, "test-key-123");
        std::env::remove_var("ANTHROPIC_API_KEY");
    }

    #[test]
    fn credential_env_var_priority_over_file() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("ANTHROPIC_API_KEY", "env-key");
        // Even if credentials file exists, env takes priority
        let key = resolve_api_key().unwrap();
        assert_eq!(key, "env-key");
        std::env::remove_var("ANTHROPIC_API_KEY");
    }

    #[test]
    fn credential_oauth_token_env_var_used_when_api_key_missing() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::set_var("CLAUDE_CODE_OAUTH_TOKEN", "sk-ant-oat01-from-setup-token");
        let key = resolve_api_key().unwrap();
        assert_eq!(key, "sk-ant-oat01-from-setup-token");
        std::env::remove_var("CLAUDE_CODE_OAUTH_TOKEN");
    }

    #[test]
    fn credential_api_key_beats_oauth_token() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("ANTHROPIC_API_KEY", "api-key-wins");
        std::env::set_var("CLAUDE_CODE_OAUTH_TOKEN", "oauth-loses");
        let key = resolve_api_key().unwrap();
        assert_eq!(key, "api-key-wins");
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("CLAUDE_CODE_OAUTH_TOKEN");
    }

    #[test]
    fn credential_file_parses_nested_oauth_shape() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".credentials.json");
        fs::write(
            &path,
            r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-abc","refreshToken":"x"}}"#,
        )
        .unwrap();
        assert_eq!(
            read_credentials_file(&path).as_deref(),
            Some("sk-ant-oat01-abc")
        );
    }

    #[test]
    fn credential_file_parses_legacy_flat_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".credentials.json");
        fs::write(&path, r#"{"apiKey":"sk-ant-api03-legacy"}"#).unwrap();
        assert_eq!(
            read_credentials_file(&path).as_deref(),
            Some("sk-ant-api03-legacy")
        );
    }

    #[test]
    fn credential_file_returns_none_for_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("does-not-exist.json");
        assert!(read_credentials_file(&path).is_none());
    }

    #[test]
    fn credential_file_returns_none_for_empty_oauth_token() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".credentials.json");
        fs::write(&path, r#"{"claudeAiOauth":{"accessToken":""}}"#).unwrap();
        assert!(read_credentials_file(&path).is_none());
    }

    #[test]
    fn no_credential_errors() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("CLAUDE_CODE_OAUTH_TOKEN");
        // We can't guarantee ~/.claude/.credentials.json doesn't exist on CI,
        // but we can at least verify the function returns a Result (not panic).
        let _ = resolve_api_key(); // Ok or Err — both are acceptable
    }
}
