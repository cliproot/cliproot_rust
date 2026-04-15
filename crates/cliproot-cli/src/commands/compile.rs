//! `cliproot compile` — CLI wrapper around [`knowledge::compile::run_compile`].
//!
//! Foreground by default.  When `--background` is passed we detach via the
//! shared [`background::spawn`] helper — useful for chaining from other
//! processes or running as a scheduled task without holding a terminal.

use std::path::PathBuf;

use crate::commands::background;
use crate::knowledge::compile::{self, CompileOutcome, CompileTrigger};

pub fn run(
    cliproot_dir_override: Option<PathBuf>,
    run_in_background: bool,
    is_background_child: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    // Resolve .cliproot/ up front — we need it in both branches.
    let cliproot_dir = match cliproot_dir_override.clone() {
        Some(p) => p,
        None => {
            let cwd = std::env::current_dir()?;
            discover_cliproot_dir(&cwd).ok_or("no .cliproot/ directory found in any ancestor")?
        }
    };

    if run_in_background && !is_background_child {
        let dir_str = cliproot_dir
            .to_str()
            .ok_or("cliproot dir path is not valid UTF-8")?;
        background::spawn(
            &["compile", "--background-child", "--cliproot-dir", dir_str],
            "cliproot-compile",
        )?;
        return Ok(());
    }

    let project_root = cliproot_dir
        .parent()
        .ok_or("cannot determine project root from .cliproot/")?;
    let repo = cliproot_store::Repository::open(project_root)?;

    let outcome = compile::run_compile(&cliproot_dir, &repo, CompileTrigger::Manual);

    // Print a single-line summary to stderr; exit 0 for all outcomes except
    // hard errors.  Budget exceeded and Skipped are not failures.
    eprintln!("cliproot compile: {outcome}");
    match outcome {
        CompileOutcome::Error(e) => Err(e.into()),
        _ => Ok(()),
    }
}

fn discover_cliproot_dir(cwd: &std::path::Path) -> Option<PathBuf> {
    let mut dir = cwd.to_path_buf();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_walks_up() {
        let dir = tempfile::tempdir().unwrap();
        let cliproot = dir.path().join(".cliproot");
        std::fs::create_dir_all(&cliproot).unwrap();
        let deep = dir.path().join("a/b/c");
        std::fs::create_dir_all(&deep).unwrap();
        assert_eq!(discover_cliproot_dir(&deep), Some(cliproot));
    }

    #[test]
    fn discover_returns_none_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(discover_cliproot_dir(dir.path()), None);
    }
}
