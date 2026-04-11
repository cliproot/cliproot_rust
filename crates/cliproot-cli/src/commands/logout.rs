use cliproot_registry::credential;
use cliproot_store::Repository;

use crate::OutputFormat;

pub fn run(remote: Option<&str>, format: &OutputFormat) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    let (_remote_name, remote_config) = super::remote::resolve_remote(&repo, remote)?;
    let registry_url = &remote_config.url;

    credential::delete_token(registry_url).map_err(|e| format!("failed to delete token: {e}"))?;

    match format {
        OutputFormat::Json => println!(r#"{{"status":"ok","message":"Logged out"}}"#),
        _ => println!("Logged out from {registry_url}"),
    }

    Ok(())
}
