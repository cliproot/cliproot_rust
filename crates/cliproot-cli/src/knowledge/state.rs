use std::collections::HashMap;
use std::fs;
use std::path::Path;

// ── FlushState ────────────────────────────────────────────────────────────────

/// Persisted at `.cliproot/knowledge/state.json`.
/// Tracks budget consumption and watermarks so flush is idempotent and
/// respects per-day token/cost caps.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct FlushState {
    /// Local date (YYYY-MM-DD) when the daily budget counters were last reset.
    pub budget_date: Option<String>,

    /// Total input+output tokens spent on flush today.
    pub daily_total_tokens: u64,

    /// Total USD cost spent on flush today.
    pub daily_total_cost_usd: f64,

    /// Per-session JSONL line counts at the time of the last successful flush.
    /// Key: session_id (filename stem of the .jsonl file).
    /// Value: number of non-blank lines seen at last flush.
    pub last_flushed_line_counts: HashMap<String, u64>,

    /// SHA-256 hex of the last written daily digest body (for idempotency).
    pub last_daily_hash: Option<String>,

    /// SHA-256 hex fingerprint of the knowledge corpus (sorted article paths +
    /// content hashes, plus today's daily hash) at the end of the most recent
    /// successful compile run.  Phase D — gates `cliproot wiki compile` so repeated
    /// runs against an unchanged corpus are no-ops.
    #[serde(default)]
    pub last_compile_hash: Option<String>,
}

impl FlushState {
    /// Returns true if the compile idempotency key has changed since the last
    /// successful compile.  Always true when no compile has ever run.
    pub fn needs_compile(&self, current_corpus_hash: &str) -> bool {
        match &self.last_compile_hash {
            Some(prev) => prev != current_corpus_hash,
            None => true,
        }
    }
}

// ── I/O helpers ───────────────────────────────────────────────────────────────

/// Load `FlushState` from `<knowledge_dir>/state.json`.
/// Returns a default (zero) state if the file does not yet exist.
pub fn load(knowledge_dir: &Path) -> Result<FlushState, Box<dyn std::error::Error>> {
    let path = knowledge_dir.join("state.json");
    if !path.exists() {
        return Ok(FlushState::default());
    }
    let json = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&json)?)
}

/// Persist `FlushState` to `<knowledge_dir>/state.json`.
pub fn save(state: &FlushState, knowledge_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(knowledge_dir)?;
    let path = knowledge_dir.join("state.json");
    fs::write(&path, serde_json::to_string_pretty(state)?)?;
    Ok(())
}

/// If the stored `budget_date` differs from today's local date, reset the
/// daily token/cost counters.  Call this at the start of every flush run.
pub fn reset_budget_if_new_day(state: &mut FlushState) {
    let today = today_local();
    match &state.budget_date {
        Some(d) if d == &today => {} // same day — keep counters
        _ => {
            state.budget_date = Some(today);
            state.daily_total_tokens = 0;
            state.daily_total_cost_usd = 0.0;
        }
    }
}

/// Count the total number of JSONL log lines across all sessions that have NOT
/// yet been flushed (i.e. lines written after the last-flushed watermarks).
///
/// Globs `<log_dir>/*.jsonl`, counts non-blank lines per file, then subtracts
/// the watermark values stored in `state.last_flushed_line_counts`.
pub fn total_new_lines(
    state: &FlushState,
    log_dir: &Path,
) -> Result<u64, Box<dyn std::error::Error>> {
    let mut total: u64 = 0;

    let entries = match fs::read_dir(log_dir) {
        Ok(e) => e,
        Err(_) => return Ok(0),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(ext) = path.extension() else {
            continue;
        };
        if ext != "jsonl" {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        // Skip watermark / marker files — only count session logs
        if stem.starts_with("watermark-") || stem.starts_with("precompact-hinted-") {
            continue;
        }

        let content = fs::read_to_string(&path).unwrap_or_default();
        let line_count = content.lines().filter(|l| !l.trim().is_empty()).count() as u64;
        let watermark = state
            .last_flushed_line_counts
            .get(stem)
            .copied()
            .unwrap_or(0);
        total += line_count.saturating_sub(watermark);
    }

    Ok(total)
}

/// Return today's local date as a `YYYY-MM-DD` string.
fn today_local() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_is_zero() {
        let s = FlushState::default();
        assert_eq!(s.daily_total_tokens, 0);
        assert_eq!(s.daily_total_cost_usd, 0.0);
        assert!(s.budget_date.is_none());
        assert!(s.last_flushed_line_counts.is_empty());
    }

    #[test]
    fn roundtrip_state() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = FlushState {
            daily_total_tokens: 12_345,
            daily_total_cost_usd: 0.12,
            ..Default::default()
        };
        state
            .last_flushed_line_counts
            .insert("abc123".to_string(), 50);

        save(&state, dir.path()).unwrap();
        let loaded = load(dir.path()).unwrap();

        assert_eq!(loaded.daily_total_tokens, 12_345);
        assert_eq!(loaded.daily_total_cost_usd, 0.12);
        assert_eq!(*loaded.last_flushed_line_counts.get("abc123").unwrap(), 50);
    }

    #[test]
    fn load_missing_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let state = load(dir.path()).unwrap();
        assert_eq!(state.daily_total_tokens, 0);
    }

    #[test]
    fn reset_budget_on_new_day() {
        let mut state = FlushState {
            budget_date: Some("2000-01-01".to_string()),
            daily_total_tokens: 9999,
            daily_total_cost_usd: 9.99,
            ..Default::default()
        };
        reset_budget_if_new_day(&mut state);
        // Today != "2000-01-01" so counters should reset
        assert_eq!(state.daily_total_tokens, 0);
        assert_eq!(state.daily_total_cost_usd, 0.0);
        // budget_date updated to today
        assert_eq!(state.budget_date, Some(today_local()));
    }

    #[test]
    fn reset_budget_same_day_unchanged() {
        let today = today_local();
        let mut state = FlushState {
            budget_date: Some(today.clone()),
            daily_total_tokens: 500,
            daily_total_cost_usd: 0.05,
            ..Default::default()
        };
        reset_budget_if_new_day(&mut state);
        assert_eq!(state.daily_total_tokens, 500);
        assert_eq!(state.daily_total_cost_usd, 0.05);
    }

    #[test]
    fn total_new_lines_counts_delta() {
        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path().join("agent-log");
        fs::create_dir_all(&log_dir).unwrap();

        // Write 5 lines to a session log
        fs::write(
            log_dir.join("session-abc.jsonl"),
            "{\"tool\":\"Read\"}\n{}\n{}\n{}\n{}\n",
        )
        .unwrap();

        // Watermark says we already processed 2
        let mut state = FlushState::default();
        state
            .last_flushed_line_counts
            .insert("session-abc".to_string(), 2);

        let new = total_new_lines(&state, &log_dir).unwrap();
        assert_eq!(new, 3); // 5 - 2
    }

    #[test]
    fn total_new_lines_no_watermark() {
        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path().join("agent-log");
        fs::create_dir_all(&log_dir).unwrap();

        fs::write(log_dir.join("sess.jsonl"), "{}\n{}\n{}\n").unwrap();

        let state = FlushState::default();
        let new = total_new_lines(&state, &log_dir).unwrap();
        assert_eq!(new, 3);
    }

    #[test]
    fn total_new_lines_skips_watermark_files() {
        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path().join("agent-log");
        fs::create_dir_all(&log_dir).unwrap();

        fs::write(log_dir.join("sess.jsonl"), "{}\n").unwrap();
        // Watermark file — should not be counted
        fs::write(log_dir.join("watermark-sess.jsonl"), "{\"line_count\":5}\n").unwrap();

        let state = FlushState::default();
        let new = total_new_lines(&state, &log_dir).unwrap();
        assert_eq!(new, 1); // only sess.jsonl counted
    }

    #[test]
    fn total_new_lines_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path().join("agent-log");
        fs::create_dir_all(&log_dir).unwrap();
        let state = FlushState::default();
        assert_eq!(total_new_lines(&state, &log_dir).unwrap(), 0);
    }

    #[test]
    fn total_new_lines_missing_dir() {
        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path().join("no-such-dir");
        let state = FlushState::default();
        assert_eq!(total_new_lines(&state, &log_dir).unwrap(), 0);
    }

    #[test]
    fn needs_compile_true_when_never_run() {
        let state = FlushState::default();
        assert!(state.needs_compile("any-hash"));
    }

    #[test]
    fn needs_compile_true_on_hash_change() {
        let state = FlushState {
            last_compile_hash: Some("old".to_string()),
            ..Default::default()
        };
        assert!(state.needs_compile("new"));
    }

    #[test]
    fn needs_compile_false_on_match() {
        let state = FlushState {
            last_compile_hash: Some("same".to_string()),
            ..Default::default()
        };
        assert!(!state.needs_compile("same"));
    }

    #[test]
    fn last_compile_hash_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let state = FlushState {
            last_compile_hash: Some("abc123".to_string()),
            ..Default::default()
        };
        save(&state, dir.path()).unwrap();
        let loaded = load(dir.path()).unwrap();
        assert_eq!(loaded.last_compile_hash.as_deref(), Some("abc123"));
    }
}
