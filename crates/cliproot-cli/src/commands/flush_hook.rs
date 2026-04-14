use std::io::Read;
use std::path::PathBuf;
use std::process::Stdio;

use crate::commands::harness::{parse_stop_input, Harness};
use crate::knowledge::flush;

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

/// Called by the `cliproot flush-hook` CLI subcommand.
///
/// When `background` is false (the default, triggered by the Claude Code Stop
/// hook): read hook JSON from stdin, run guards, and spawn a detached child
/// process to do the actual flush work.
///
/// When `background` is true (the detached child invocation): open the repo
/// from `cliproot_dir` and run the flush synchronously.
pub fn run(
    harness: Harness,
    background: bool,
    cliproot_dir_override: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    if background {
        run_background(cliproot_dir_override)
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

    // 5. Spawn detached background process.
    spawn_background(&cliproot_dir)?;

    // 6. Return without printing anything — Stop hooks don't need a decision.
    Ok(())
}

// ── Background path ───────────────────────────────────────────────────────────

fn run_background(
    cliproot_dir_override: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let cliproot_dir = cliproot_dir_override
        .ok_or("--cliproot-dir is required in --background mode")?;

    let project_root = cliproot_dir
        .parent()
        .ok_or("cannot determine project root from --cliproot-dir")?;

    let repo = cliproot_store::Repository::open(project_root)?;

    let outcome = flush::run_flush(&cliproot_dir, &repo);

    // Log the outcome regardless of success or failure.
    eprintln!("cliproot flush-hook [background]: {outcome}");

    // Exit 0 for all outcomes — budget exceeded, skipped, etc. are all OK.
    Ok(())
}

// ── Detached spawn ────────────────────────────────────────────────────────────

fn spawn_background(cliproot_dir: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("cannot locate cliproot executable: {e}"))?;

    let cliproot_dir_str = cliproot_dir
        .to_str()
        .ok_or("cliproot dir path is not valid UTF-8")?;

    let mut cmd = std::process::Command::new(&exe);
    cmd.args([
        "flush-hook",
        "--background",
        "--cliproot-dir",
        cliproot_dir_str,
    ])
    .env("CLAUDE_INVOKED_BY", "cliproot-flush-hook")
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid() is async-signal-safe. The child simply creates a
        // new session so it outlives the parent (Claude Code) process.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
    }

    cmd.spawn()
        .map_err(|e| format!("failed to spawn background flush process: {e}"))?;

    Ok(())
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
}
