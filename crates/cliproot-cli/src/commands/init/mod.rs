mod agent_config;

use cliproot_store::{Repository, StoreError};

pub fn run(agent: bool) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;

    match Repository::init(&cwd) {
        Ok(_) => println!("Initialized .cliproot/ repository in {}", cwd.display()),
        Err(StoreError::AlreadyExists(_)) if agent => {
            // .cliproot/ already exists — that's fine when generating agent configs
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

    Ok(())
}
