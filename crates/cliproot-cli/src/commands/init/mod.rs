mod agent_config;
mod hook_config;

pub use hook_config::HarnessSelection;

use cliproot_store::{Repository, StoreError};

pub fn run(
    agent: bool,
    hooks: bool,
    hooks_for: Option<HarnessSelection>,
) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;

    match Repository::init(&cwd) {
        Ok(_) => println!("Initialized .cliproot/ repository in {}", cwd.display()),
        Err(StoreError::AlreadyExists(_)) if agent || hooks => {
            // .cliproot/ already exists — that's fine when generating configs or hooks
        }
        Err(e) => return Err(e.into()),
    }

    if agent {
        let actions = agent_config::generate_all(&cwd)?;
        for action in &actions {
            println!("  {} {}", action.symbol(), action.path_display(&cwd));
        }
        println!(
            "\nAgent configuration complete. {} files written.",
            actions.len()
        );
        agent_config::print_codex_instructions();
    }

    if hooks {
        let selection = hooks_for.unwrap_or(HarnessSelection::Auto);
        let actions = hook_config::install_hooks_for(&cwd, selection)?;

        for action in &actions {
            println!("  {} {}", action.symbol(), action.path_display(&cwd));
        }

        // Ensure agent-log directory exists
        let log_dir = cwd.join(".cliproot/agent-log");
        std::fs::create_dir_all(&log_dir)?;

        if actions.len() > 1 {
            println!(
                "\nPostToolUse + Stop + PreCompact hooks installed for {} harnesses.",
                actions.len()
            );
        } else {
            println!("\nPostToolUse + Stop + PreCompact hooks installed.");
        }
    }

    Ok(())
}
