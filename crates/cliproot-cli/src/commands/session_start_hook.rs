//! `cliproot session-start-hook` — Claude Code SessionStart injection.
//!
//! Phase D.  Reads `.cliproot/knowledge/index.md` and the most recent
//! `daily/*.md`, builds a short (~5 KB) plaintext block, and emits it to
//! stdout as Claude Code's `hookSpecificOutput.additionalContext`.
//!
//! # Guarantees
//!
//! - **Never blocks a session.** All file I/O runs inside a 500 ms hard
//!   timeout; on miss we print `{}` and exit 0.
//! - **Never throws.** Any error inside the worker emits `{}` to stdout with
//!   the reason logged to stderr (visible in Claude Code's hook transcript).
//! - **Budget-capped reads.** `File::take(budget * 2)` ensures we never read
//!   more than ~10 KB of markdown per file even if someone has a 1 GB
//!   `index.md`.
//!
//! # Output shape (Claude Code contract)
//!
//! ```json
//! {"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": "..."}}
//! ```
//!
//! When multiple SessionStart hooks fire, Claude Code concatenates their
//! `additionalContext` values — ours is self-contained and safe to stack.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use crate::commands::harness::Harness;

const TIMEOUT: Duration = Duration::from_millis(500);

/// Public entry point called from `main.rs`.
///
/// `harness` is accepted for signature parity with other hook commands even
/// though SessionStart is Claude-only today.  `cliproot_dir_override` is for
/// tests.
pub fn run(harness: Harness, cliproot_dir_override: Option<PathBuf>) {
    // SessionStart is Claude-only today — but if some future harness wires us
    // up we'll still respond (empty passthrough is always safe).
    let _ = harness;

    // Run the worker in a thread with a hard timeout.  On timeout, error, or
    // any other failure, emit `{}` — the session must never be blocked.
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        let out = match build_context(cliproot_dir_override) {
            Ok(Some(ctx)) => render_output(&ctx),
            Ok(None) => "{}".to_string(),
            Err(e) => {
                eprintln!("cliproot session-start-hook: {e}");
                "{}".to_string()
            }
        };
        let _ = tx.send(out);
    });

    let out = match rx.recv_timeout(TIMEOUT) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("cliproot session-start-hook: TIMEOUT after {TIMEOUT:?}");
            "{}".to_string()
        }
    };
    println!("{out}");
}

// ── Stdin payload ─────────────────────────────────────────────────────────────

/// Minimum viable decoding of the SessionStart input — we only need `cwd`.
#[derive(serde::Deserialize, Default)]
struct SessionStartInput {
    #[serde(default)]
    cwd: String,
    // session_id, source, hook_event_name are accepted silently.
    #[serde(flatten)]
    _extra: serde_json::Value,
}

fn read_stdin_cwd() -> Result<String, Box<dyn std::error::Error>> {
    let mut raw = String::new();
    // Bound the read — stdin is nominally small JSON.  16 KiB is plenty.
    std::io::stdin().take(16 * 1024).read_to_string(&mut raw)?;
    if raw.trim().is_empty() {
        // No stdin → fall back to CWD env.
        return std::env::current_dir()
            .map(|p| p.display().to_string())
            .map_err(Into::into);
    }
    let parsed: SessionStartInput = serde_json::from_str(&raw).unwrap_or_default();
    if parsed.cwd.is_empty() {
        std::env::current_dir()
            .map(|p| p.display().to_string())
            .map_err(Into::into)
    } else {
        Ok(parsed.cwd)
    }
}

// ── Worker ────────────────────────────────────────────────────────────────────

fn build_context(
    cliproot_dir_override: Option<PathBuf>,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let cliproot_dir = match cliproot_dir_override {
        Some(p) => p,
        None => {
            let cwd = read_stdin_cwd()?;
            match discover_cliproot_dir(&cwd) {
                Some(p) => p,
                None => return Ok(None), // No .cliproot/ → fail-open.
            }
        }
    };

    let project_root = cliproot_dir
        .parent()
        .ok_or("cannot determine project root from .cliproot/")?;
    let repo = cliproot_store::Repository::open(project_root)?;
    let cfg = repo.knowledge_config()?;
    if !cfg.level.allows_session_start() {
        return Ok(None);
    }

    let budget = cfg.session_start_inject_budget_chars.max(256);
    let knowledge_dir = cliproot_dir.join("knowledge");

    let index_excerpt = read_index_excerpt(&knowledge_dir, budget);
    let daily_excerpt = read_daily_headings(&knowledge_dir, budget);

    if index_excerpt.is_none() && daily_excerpt.is_none() {
        return Ok(None);
    }

    let mut out = String::new();
    out.push_str("## cliproot wiki snapshot\n\n");
    if let Some(idx) = index_excerpt {
        out.push_str("### index.md\n");
        out.push_str(idx.trim());
        out.push_str("\n\n");
    }
    if let Some(daily) = daily_excerpt {
        out.push_str("### latest daily digest headings\n");
        out.push_str(daily.trim());
        out.push('\n');
    }

    // Enforce the char budget.
    if out.chars().count() > budget {
        let truncated: String = out.chars().take(budget).collect();
        out = truncated;
    }

    Ok(Some(out))
}

fn render_output(additional_context: &str) -> String {
    serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "SessionStart",
            "additionalContext": additional_context,
        }
    })
    .to_string()
}

// ── `.cliproot/` discovery ────────────────────────────────────────────────────

fn discover_cliproot_dir(cwd: &str) -> Option<PathBuf> {
    let mut dir = PathBuf::from(cwd);
    loop {
        let candidate = dir.join(".cliproot");
        if candidate.is_dir() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

// ── Bounded reads ─────────────────────────────────────────────────────────────

fn read_bounded(path: &Path, cap_bytes: u64) -> Option<String> {
    let file = File::open(path).ok()?;
    let mut buf = String::new();
    file.take(cap_bytes).read_to_string(&mut buf).ok()?;
    Some(buf)
}

fn read_index_excerpt(knowledge_dir: &Path, budget_chars: usize) -> Option<String> {
    let cap = (budget_chars as u64).saturating_mul(2);
    let raw = read_bounded(&knowledge_dir.join("index.md"), cap)?;
    // Keep frontmatter + header row + up to N body rows.  We cap lines rather
    // than re-parsing the table.
    let mut out = String::new();
    let mut rows = 0usize;
    let mut saw_header = false;
    let mut saw_separator = false;
    for line in raw.lines() {
        if !line.trim_start().starts_with('|') {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        // Table line.
        if !saw_header {
            out.push_str(line);
            out.push('\n');
            if line.contains("concept") && line.contains("uuid") {
                saw_header = true;
            }
            continue;
        }
        if !saw_separator {
            out.push_str(line);
            out.push('\n');
            if line.contains("---") {
                saw_separator = true;
            }
            continue;
        }
        // Real data row.
        if rows >= 40 {
            out.push_str("... (truncated)\n");
            break;
        }
        out.push_str(line);
        out.push('\n');
        rows += 1;
    }
    Some(out)
}

fn read_daily_headings(knowledge_dir: &Path, budget_chars: usize) -> Option<String> {
    let daily_dir = knowledge_dir.join("daily");
    let entries = std::fs::read_dir(&daily_dir).ok()?;
    let mut dailies: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("md"))
        .collect();
    dailies.sort();
    let latest = dailies.pop()?;

    let cap = (budget_chars as u64).saturating_mul(2);
    let raw = read_bounded(&latest, cap)?;

    let mut out = String::new();
    out.push_str(&format!(
        "source: daily/{}\n",
        latest
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("latest.md")
    ));

    // Preserve YAML frontmatter as-is; then emit only `##`/`###` lines.
    let mut in_frontmatter = false;
    for (idx, line) in raw.lines().enumerate() {
        if idx == 0 && line.trim() == "---" {
            in_frontmatter = true;
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if in_frontmatter {
            out.push_str(line);
            out.push('\n');
            if line.trim() == "---" {
                in_frontmatter = false;
            }
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with("## ") || trimmed.starts_with("### ") {
            out.push_str(line);
            out.push('\n');
        }
    }
    Some(out)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_output_shape_matches_claude_code_contract() {
        let out = render_output("hello");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["hookSpecificOutput"]["hookEventName"], "SessionStart");
        assert_eq!(v["hookSpecificOutput"]["additionalContext"], "hello");
    }

    #[test]
    fn discover_walks_up() {
        let dir = tempfile::tempdir().unwrap();
        let cliproot = dir.path().join(".cliproot");
        std::fs::create_dir_all(&cliproot).unwrap();
        let deep = dir.path().join("a/b/c");
        std::fs::create_dir_all(&deep).unwrap();
        assert_eq!(
            discover_cliproot_dir(deep.to_str().unwrap()),
            Some(cliproot)
        );
    }

    #[test]
    fn discover_returns_none_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(discover_cliproot_dir(dir.path().to_str().unwrap()), None);
    }

    #[test]
    fn read_bounded_caps_reads() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("big.md");
        std::fs::write(&p, "a".repeat(10_000)).unwrap();
        let out = read_bounded(&p, 100).unwrap();
        assert_eq!(out.len(), 100);
    }

    #[test]
    fn read_index_excerpt_respects_row_cap() {
        let dir = tempfile::tempdir().unwrap();
        let kd = dir.path().to_path_buf();
        let mut body = String::from("---\nschemaVersion: 1\n---\n\n# Wiki index\n\n");
        body.push_str("| concept | uuid | type | tags | last_seen |\n");
        body.push_str("|---|---|---|---|---|\n");
        for i in 0..80 {
            body.push_str(&format!("| T{i} | u{i} | concept | | 2026-04-13 |\n"));
        }
        std::fs::write(kd.join("index.md"), body).unwrap();
        let out = read_index_excerpt(&kd, 5_000).unwrap();
        assert!(out.contains("(truncated)"));
    }

    #[test]
    fn read_daily_headings_keeps_frontmatter_and_h2() {
        let dir = tempfile::tempdir().unwrap();
        let kd = dir.path().to_path_buf();
        std::fs::create_dir_all(kd.join("daily")).unwrap();
        let body = "---\nuuid: abc\ndate: 2026-04-13\n---\n\n\
                    ## Summary\nbody text here should not appear\n\n\
                    ## Decisions\nmore body\n\n\
                    ### sub heading\n- item\n";
        std::fs::write(kd.join("daily/2026-04-13.md"), body).unwrap();
        let out = read_daily_headings(&kd, 5_000).unwrap();
        assert!(out.contains("uuid: abc"), "frontmatter preserved: {out}");
        assert!(out.contains("## Summary"));
        assert!(out.contains("## Decisions"));
        assert!(out.contains("### sub heading"));
        assert!(!out.contains("body text here"), "bodies filtered: {out}");
    }

    #[test]
    fn build_context_fail_open_when_no_cliproot() {
        let dir = tempfile::tempdir().unwrap();
        // Point at a directory with no .cliproot/ — override isn't set, so it
        // will try stdin; but since stdin is empty in cfg(test), we instead
        // exercise the None branch directly by passing a non-existent path
        // through an override and handling the error:
        let bogus = dir.path().join(".cliproot-never");
        let res = build_context(Some(bogus));
        assert!(res.is_err() || matches!(res, Ok(None)));
    }
}
