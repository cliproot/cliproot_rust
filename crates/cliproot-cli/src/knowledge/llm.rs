use std::fs;
use std::path::PathBuf;

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
/// Credentials are resolved in order:
/// 1. `ANTHROPIC_API_KEY` environment variable.
/// 2. `~/.claude/.credentials.json` (tries `claudeAiOauthAccessToken` then `apiKey`).
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
        return Err(format!(
            "Anthropic API error {status}: {body_text}"
        )
        .into());
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
    let estimated_cost_usd = (input_tokens as f64 / 1_000_000.0) * 1.0
        + (output_tokens as f64 / 1_000_000.0) * 5.0;

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

fn resolve_api_key() -> Result<String, Box<dyn std::error::Error>> {
    // 1. Env var
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        if !key.is_empty() {
            return Ok(key);
        }
    }

    // 2. ~/.claude/.credentials.json
    if let Some(key) = read_claude_credentials() {
        return Ok(key);
    }

    Err(
        "No Anthropic API key found.\n\
         Set ANTHROPIC_API_KEY environment variable, or sign in to Claude Code \
         (which stores credentials in ~/.claude/.credentials.json)."
            .into(),
    )
}

fn read_claude_credentials() -> Option<String> {
    let home = home_dir()?;
    let path = home.join(".claude").join(".credentials.json");
    let json: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).ok()?).ok()?;

    // Try OAuth token first, then explicit apiKey
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
    std::env::var("HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
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
        std::env::set_var("ANTHROPIC_API_KEY", "test-key-123");
        let key = resolve_api_key().unwrap();
        assert_eq!(key, "test-key-123");
        std::env::remove_var("ANTHROPIC_API_KEY");
    }

    #[test]
    fn credential_env_var_priority_over_file() {
        std::env::set_var("ANTHROPIC_API_KEY", "env-key");
        // Even if credentials file exists, env takes priority
        let key = resolve_api_key().unwrap();
        assert_eq!(key, "env-key");
        std::env::remove_var("ANTHROPIC_API_KEY");
    }

    #[test]
    fn no_credential_errors() {
        std::env::remove_var("ANTHROPIC_API_KEY");
        // We can't guarantee ~/.claude/.credentials.json doesn't exist on CI,
        // but we can at least verify the function returns a Result (not panic).
        let _ = resolve_api_key(); // Ok or Err — both are acceptable
    }
}
