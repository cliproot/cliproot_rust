use cliproot_registry::RegistryClient;
use cliproot_store::Repository;

use crate::OutputFormat;

pub fn run(
    project: Option<&str>,
    remote: Option<&str>,
    format: &OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    let (_remote_name, remote_config) = super::remote::resolve_remote(&repo, remote)?;

    let owner = remote_config.owner.as_deref().ok_or(
        "remote has no owner configured; use `cliproot remote add <name> <url> --owner <owner>`",
    )?;

    let project_name = project
        .map(String::from)
        .or_else(|| repo.current_project_id().ok().flatten())
        .ok_or("no project specified and no current project set")?;

    // Fetch the pack from the registry.
    let client = RegistryClient::new(&remote_config.url)?;
    let pack_path = client.pull_pack(owner, &project_name)?;

    // Import the pack into the local repo.
    let manifest = repo.import_pack(&pack_path, None)?;

    // Clean up the temp file.
    let _ = std::fs::remove_file(&pack_path);

    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&manifest)?),
        _ => {
            println!("Pulled {} clips, {} artifacts", manifest.counts.clips, manifest.counts.artifacts);
            if let Some(project) = &manifest.project {
                println!("  project: {} ({})", project.name, project.id);
            }
        }
    }
    Ok(())
}
