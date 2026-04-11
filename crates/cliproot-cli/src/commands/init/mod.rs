mod agent_config;
mod hook_config;

use cliproot_store::{Repository, StoreError};

pub fn run(agent: bool, hooks: bool) -> Result<(), Box<dyn std::error::Error>> {
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
        let action = hook_config::install_hooks(&cwd)?;
        println!("  {} {}", action.symbol(), action.path_display(&cwd));

        // Ensure agent-log directory exists
        let log_dir = cwd.join(".cliproot/agent-log");
        std::fs::create_dir_all(&log_dir)?;
        println!("\nPostToolUse + Stop + PreCompact hooks installed.");
    }

    Ok(())
}
