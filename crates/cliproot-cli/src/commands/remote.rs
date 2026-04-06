use cliproot_store::{RemoteConfig, Repository};

use crate::OutputFormat;

pub fn add(
    name: &str,
    url: &str,
    owner: Option<&str>,
    format: &OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    repo.add_remote(name, url, owner)?;
    match format {
        OutputFormat::Json => {
            let info = serde_json::json!({
                "name": name,
                "url": url,
                "owner": owner,
            });
            println!("{}", serde_json::to_string_pretty(&info)?);
        }
        _ => {
            println!("Added remote {name} → {url}");
            if let Some(o) = owner {
                println!("  owner: {o}");
            }
        }
    }
    Ok(())
}

pub fn remove(name: &str, format: &OutputFormat) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    repo.remove_remote(name)?;
    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::json!({ "removed": name }));
        }
        _ => println!("Removed remote {name}"),
    }
    Ok(())
}

pub fn list(format: &OutputFormat) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    let remotes = repo.list_remotes()?;
    let default = repo.default_remote()?;
    let default_name = default.map(|(n, _)| n);

    match format {
        OutputFormat::Json => {
            let arr: Vec<_> = remotes
                .iter()
                .map(|(name, cfg)| {
                    serde_json::json!({
                        "name": name,
                        "url": cfg.url,
                        "owner": cfg.owner,
                        "default": default_name.as_deref() == Some(name.as_str()),
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&arr)?);
        }
        _ => {
            if remotes.is_empty() {
                println!("No remotes configured.");
            } else {
                for (name, cfg) in &remotes {
                    let star = if default_name.as_deref() == Some(name.as_str()) {
                        "* "
                    } else {
                        "  "
                    };
                    let owner_str = cfg
                        .owner
                        .as_ref()
                        .map(|o| format!(" (owner: {o})"))
                        .unwrap_or_default();
                    println!("{star}{name}\t{}{owner_str}", cfg.url);
                }
            }
        }
    }
    Ok(())
}

/// Resolve a remote by name, or fall back to the default remote.
pub fn resolve_remote(
    repo: &Repository,
    name: Option<&str>,
) -> Result<(String, RemoteConfig), Box<dyn std::error::Error>> {
    match name {
        Some(n) => {
            let config = repo.get_remote(n)?;
            Ok((n.to_string(), config))
        }
        None => repo
            .default_remote()?
            .ok_or_else(|| "no remote configured; use `cliproot remote add <name> <url>`".into()),
    }
}
