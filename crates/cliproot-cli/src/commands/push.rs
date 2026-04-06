use cliproot_registry::RegistryClient;
use cliproot_store::Repository;

use crate::OutputFormat;

pub fn run(
    project: Option<&str>,
    remote: Option<&str>,
    format: &OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;

    let project_id = project
        .map(String::from)
        .or_else(|| repo.current_project_id().ok().flatten())
        .ok_or("no project specified and no current project set")?;

    let (remote_name, remote_config) = super::remote::resolve_remote(&repo, remote)?;

    // Create a pack in a temp file.
    let tmp = tempfile::NamedTempFile::new()?;
    let _manifest = repo.create_pack(Some(&project_id), &[], None, tmp.path())?;

    // Push to the registry.
    let client = RegistryClient::new(&remote_config.url)?;
    let result = client.push_pack(tmp.path())?;

    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&result)?),
        _ => {
            println!("Pushed project {} to {}", project_id, remote_name);
            println!("  pack:      {}", result.pack_hash);
            println!("  clips:     {}", result.clips);
            println!("  artifacts: {}", result.artifacts);
            println!("  edges:     {}", result.edges);
            println!("  url:       {}", result.url);
        }
    }
    Ok(())
}
