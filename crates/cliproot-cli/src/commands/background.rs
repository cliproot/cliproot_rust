//! Detached-process spawn helper shared by `flush-hook` and `compile`.
//!
//! Starts a new copy of the current executable with the given args, fully
//! detached from the parent session so it survives the Claude Code process
//! terminating.  On Unix this uses `setsid()`; on Windows it uses
//! `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP`.

use std::process::{Command, Stdio};

/// Spawn a detached background copy of the current `cliproot` binary with the
/// given command-line arguments.
///
/// The env var `CLAUDE_INVOKED_BY` is set on the child as a recursion guard —
/// nested hooks inside the child see it and skip re-spawning.
pub fn spawn(args: &[&str], invoked_by: &str) -> Result<(), Box<dyn std::error::Error>> {
    let exe =
        std::env::current_exe().map_err(|e| format!("cannot locate cliproot executable: {e}"))?;

    let mut cmd = Command::new(&exe);
    cmd.args(args)
        .env("CLAUDE_INVOKED_BY", invoked_by)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid() is async-signal-safe.  The child creates a new
        // session so it outlives Claude Code.
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
        .map_err(|e| format!("failed to spawn detached process: {e}"))?;

    Ok(())
}
