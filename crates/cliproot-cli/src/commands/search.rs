use cliproot_registry::RegistryClient;
use cliproot_store::Repository;
use colored::Colorize;

use crate::OutputFormat;

pub fn run(
    query: &str,
    remote: Option<&str>,
    limit: u32,
    format: &OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    let (_remote_name, remote_config) = super::remote::resolve_remote(&repo, remote)?;

    let client = RegistryClient::new(&remote_config.url)?;
    let response = client.search(query, None, None, Some(limit))?;

    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&response)?),
        _ => {
            if response.results.is_empty() {
                println!("No results found.");
            } else {
                for result in &response.results {
                    let hash_short = if result.clip_hash.len() > 30 {
                        &result.clip_hash[..30]
                    } else {
                        &result.clip_hash
                    };
                    let preview = result
                        .content_preview
                        .as_deref()
                        .unwrap_or("(no preview)");
                    let preview = if preview.len() > 60 {
                        format!("{}...", &preview[..60])
                    } else {
                        preview.to_string()
                    };
                    println!("{}  {}", hash_short.dimmed(), preview);
                }
                println!("\n{} result(s) (total: {})", response.results.len(), response.total);
            }
        }
    }
    Ok(())
}
