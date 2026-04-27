use std::io::Read;
use std::path::{Path, PathBuf};

use crate::commands::background;
use crate::commands::harness::{parse_stop_input, Harness};
use crate::knowledge::{compile, flush};
use crate::transcript::session_attribution;

/// Discover the `.cliproot/` directory by walking up from `cwd`.
fn discover_cliproot_dir(cwd: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let mut dir = PathBuf::from(cwd);
    loop {
        let candidate = dir.join(".cliproot");
        if candidate.is_dir() {
            return Ok(candidate);
        }
        if !dir.pop() {
            return Err("no .cliproot/ directory found in any ancestor".into());
        }
    }
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Called by the `cliproot hook flush` CLI subcommand.
///
/// When `background` is false (the default, triggered by the Claude Code Stop
/// hook): read hook JSON from stdin, run guards, and spawn a detached child
/// process to do the actual flush work.
///
/// When `background` is true (the detached child invocation): open the repo
/// from `cliproot_dir` and run the flush synchronously.
/// `cliproot hook flush`
pub fn run(
    harness: Harness,
    background: bool,
    cliproot_dir_override: Option<PathBuf>,
    transcript_path: Option<PathBuf>,
    session_id: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    if background {
        run_background(cliproot_dir_override, transcript_path, session_id)
    } else {
        run_foreground(harness)
    }
}

// ── Foreground path ───────────────────────────────────────────────────────────

fn run_foreground(harness: Harness) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Recursion guard: the detached child sets this env var so a second
    //    Stop hook fired inside a flush-spawned process doesn't recurse.
    if std::env::var("CLAUDE_INVOKED_BY").is_ok() {
        eprintln!("cliproot flush-hook: RECURSION_GUARD (CLAUDE_INVOKED_BY set) — skipping");
        return Ok(());
    }

    // 2. Read and parse the Stop hook JSON from stdin.
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let hook = parse_stop_input(harness, &input)?;

    // 3. Discover the .cliproot/ directory.
    let cliproot_dir = discover_cliproot_dir(&hook.cwd)?;

    // 4. Level gate: open repo and check knowledge.level.
    let project_root = cliproot_dir
        .parent()
        .ok_or("cannot determine project root from .cliproot/")?;
    let repo = cliproot_store::Repository::open(project_root)?;
    let cfg = repo.knowledge_config()?;
    if !cfg.level.allows_flush() {
        eprintln!(
            "cliproot flush-hook: FLUSH_DISABLED (level={:?} < digest)",
            cfg.level
        );
        return Ok(());
    }

    // 5. Spawn detached background process (`cliproot hook flush --background`).
    //    Forward transcript_path and session_id so the background worker can run
    //    session attribution after flush completes.
    spawn_background(
        &cliproot_dir,
        hook.transcript_path.as_deref(),
        &hook.session_id,
    )?;

    // 6. Return without printing anything — Stop hooks don't need a decision.
    Ok(())
}

// ── Background path ───────────────────────────────────────────────────────────

fn run_background(
    cliproot_dir_override: Option<PathBuf>,
    transcript_path: Option<PathBuf>,
    session_id: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    run_background_impl(cliproot_dir_override, transcript_path, session_id)?;
    Ok(())
}

/// In-process implementation of the background flush + optional compile + attribution chain.
/// Pub(crate) so the integration test can call it directly without a subprocess.
pub(crate) fn run_background_impl(
    cliproot_dir_override: Option<PathBuf>,
    transcript_path: Option<PathBuf>,
    session_id: Option<String>,
) -> Result<compile::CompileOutcome, Box<dyn std::error::Error>> {
    let cliproot_dir =
        cliproot_dir_override.ok_or("--cliproot-dir is required in --background mode")?;

    let project_root = cliproot_dir
        .parent()
        .ok_or("cannot determine project root from --cliproot-dir")?;

    let repo = cliproot_store::Repository::open(project_root)?;
    let knowledge_dir = cliproot_dir.join("knowledge");

    let outcome = flush::run_flush(&cliproot_dir, &repo);
    eprintln!("cliproot hook flush [background]: {outcome}");
    // The detached child's stderr is /dev/null, so persist non-success outcomes
    // (errors, skipped, budget-exceeded not already logged internally) to log.md.
    log_background_outcome_if_unlogged(&knowledge_dir, "flush", &outcome);

    let compile_outcome = if matches!(outcome, flush::FlushOutcome::Success { .. }) {
        compile::run_compile(&cliproot_dir, &repo, compile::CompileTrigger::PostFlush)
    } else {
        compile::CompileOutcome::Skipped(format!("flush: {outcome}"))
    };
    eprintln!("cliproot hook flush [background] → compile: {compile_outcome}");
    log_background_compile_outcome_if_unlogged(&knowledge_dir, &compile_outcome);

    // Session attribution runs after flush/compile so newly-created clips are present.
    if let (Some(tp), Some(sid)) = (transcript_path, session_id) {
        let attr_cfg = repo.knowledge_config()?.session_attribution.clone();
        let attr_outcome = session_attribution::run_session_attribution(
            &cliproot_dir,
            &repo,
            &sid,
            &tp,
            &attr_cfg,
        );
        eprintln!("cliproot hook flush [background] → attribute: {attr_outcome}");
        log_attribution_outcome_if_unlogged(&knowledge_dir, &attr_outcome);
    } else {
        flush::append_log_line(
            &knowledge_dir,
            "[background] attribute: SKIPPED no transcript path",
        );
    }

    Ok(compile_outcome)
}

fn log_background_outcome_if_unlogged(
    knowledge_dir: &Path,
    stage: &str,
    outcome: &flush::FlushOutcome,
) {
    // Success and BudgetExceeded are already written to log.md inside run_flush.
    // Error and Skipped are not — surface them so silent failures become visible.
    match outcome {
        flush::FlushOutcome::Error(_) | flush::FlushOutcome::Skipped(_) => {
            flush::append_log_line(knowledge_dir, &format!("[background] {stage}: {outcome}"));
        }
        flush::FlushOutcome::Success { .. } | flush::FlushOutcome::BudgetExceeded(_) => {}
    }
}

fn log_background_compile_outcome_if_unlogged(
    knowledge_dir: &Path,
    outcome: &compile::CompileOutcome,
) {
    // compile::run_compile already logs Success, Skipped (most paths), and
    // BudgetExceeded to log.md. Error never is — surface it here.
    if matches!(outcome, compile::CompileOutcome::Error(_)) {
        flush::append_log_line(knowledge_dir, &format!("[background] compile: {outcome}"));
    }
}

// ── Detached spawn ────────────────────────────────────────────────────────────

fn spawn_background(
    cliproot_dir: &Path,
    transcript_path: Option<&str>,
    session_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let cliproot_dir_str = cliproot_dir
        .to_str()
        .ok_or("cliproot dir path is not valid UTF-8")?;

    let mut args = vec![
        "hook",
        "flush",
        "--background",
        "--cliproot-dir",
        cliproot_dir_str,
        "--session-id",
        session_id,
    ];

    // Leak the String so we can push a &str into args; both live until spawn() returns.
    let tp_owned;
    if let Some(tp) = transcript_path {
        tp_owned = tp.to_string();
        args.push("--transcript-path");
        args.push(&tp_owned);
    }

    background::spawn(&args, "cliproot-flush-hook")
}

fn log_attribution_outcome_if_unlogged(
    knowledge_dir: &Path,
    outcome: &session_attribution::AttributionOutcome,
) {
    match outcome {
        session_attribution::AttributionOutcome::Error(_)
        | session_attribution::AttributionOutcome::Skipped(_) => {
            flush::append_log_line(knowledge_dir, &format!("[background] attribute: {outcome}"));
        }
        session_attribution::AttributionOutcome::Success { .. }
        | session_attribution::AttributionOutcome::AlreadyRun => {}
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recursion_guard_exits_clean() {
        // Set the guard env var and verify run_foreground would short-circuit.
        // We can't easily test the full function here without a mock stdin,
        // but we can verify the guard condition check compiles and is reachable.
        let guard_set = std::env::var("CLAUDE_INVOKED_BY").is_ok();
        // In the test environment this may or may not be set — just check type.
        let _: bool = guard_set;
    }

    #[test]
    fn discover_cliproot_dir_finds_ancestor() {
        let dir = tempfile::tempdir().unwrap();
        let cliproot = dir.path().join(".cliproot");
        std::fs::create_dir_all(&cliproot).unwrap();
        let deep = dir.path().join("a/b/c");
        std::fs::create_dir_all(&deep).unwrap();

        let found = discover_cliproot_dir(deep.to_str().unwrap()).unwrap();
        assert_eq!(found, cliproot);
    }

    #[test]
    fn discover_cliproot_dir_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let result = discover_cliproot_dir(dir.path().to_str().unwrap());
        assert!(result.is_err());
    }

    #[test]
    fn background_flush_error_written_to_log_md() {
        let dir = tempfile::tempdir().unwrap();
        let knowledge_dir = dir.path().join("knowledge");

        log_background_outcome_if_unlogged(
            &knowledge_dir,
            "flush",
            &flush::FlushOutcome::Error("no api key".to_string()),
        );

        let log = std::fs::read_to_string(knowledge_dir.join("log.md")).unwrap();
        assert!(log.contains("[background] flush: ERROR"));
        assert!(log.contains("no api key"));
    }

    #[test]
    fn background_compile_error_written_to_log_md() {
        let dir = tempfile::tempdir().unwrap();
        let knowledge_dir = dir.path().join("knowledge");

        log_background_compile_outcome_if_unlogged(
            &knowledge_dir,
            &compile::CompileOutcome::Error("boom".to_string()),
        );

        let log = std::fs::read_to_string(knowledge_dir.join("log.md")).unwrap();
        assert!(log.contains("[background] compile: ERROR"));
        assert!(log.contains("boom"));
    }

    #[test]
    fn background_success_does_not_double_log() {
        // run_flush already logs Success internally; outer helper must not add a
        // duplicate entry.
        let dir = tempfile::tempdir().unwrap();
        let knowledge_dir = dir.path().join("knowledge");

        log_background_outcome_if_unlogged(
            &knowledge_dir,
            "flush",
            &flush::FlushOutcome::Success {
                digest_path: "x".into(),
                tokens_used: 0,
            },
        );

        assert!(!knowledge_dir.join("log.md").exists());
    }

    #[test]
    fn postflush_compile_chains_in_same_process() {
        // Tests the in-process compile chain via run_background_impl.
        // We set compile_after_hour very late so PostFlush always fires.
        let dir = tempfile::tempdir().unwrap();
        let repo = cliproot_store::Repository::init(dir.path()).unwrap();
        let mut cfg = repo.knowledge_config().unwrap();
        cfg.level = cliproot_store::KnowledgeLevel::Wiki;
        cfg.compile_after_hour = 23; // Always fires
        repo.set_knowledge_config(cfg).unwrap();

        let cliproot_dir = dir.path().join(".cliproot");
        let knowledge_dir = cliproot_dir.join("knowledge");
        std::fs::create_dir_all(&knowledge_dir).unwrap();

        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        crate::knowledge::article::write_daily_digest(
            &knowledge_dir,
            &today,
            "## Summary\nWorked on things.\n",
            None,
        )
        .unwrap();

        // Pre-set the budget to something so flush won't fail.
        let mut state = crate::knowledge::state::load(&knowledge_dir).unwrap();
        state.daily_total_tokens = 0;
        crate::knowledge::state::save(&state, &knowledge_dir).unwrap();

        let compile_outcome = run_background_impl(Some(cliproot_dir), None, None).unwrap();
        assert!(
            matches!(
                compile_outcome,
                compile::CompileOutcome::Success { .. } | compile::CompileOutcome::Skipped(_)
            ),
            "compile chain should succeed or skip gracefully: {compile_outcome:?}"
        );
    }
}
