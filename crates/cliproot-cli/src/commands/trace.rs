use cliproot_store::Repository;
use colored::Colorize;

use crate::OutputFormat;

pub fn run(hash_or_id: &str, format: &OutputFormat) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;

    let clip_hash = repo
        .resolve_clip_hash(hash_or_id)?
        .ok_or_else(|| format!("clip not found: {hash_or_id}"))?;

    let lineage = repo.trace(hash_or_id)?;

    match format {
        OutputFormat::Json => {
            let nodes: Vec<serde_json::Value> = lineage
                .iter()
                .map(|n| {
                    serde_json::json!({
                        "clipHash": n.clip_hash,
                        "parentHash": n.parent_hash,
                        "transformationType": n.transformation_type,
                        "depth": n.depth,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&nodes)?);
        }
        OutputFormat::Text | OutputFormat::Table => {
            let short = &clip_hash[..std::cmp::min(30, clip_hash.len())];
            println!("{} {}", "Lineage for".bold(), short);

            if lineage.is_empty() {
                println!("  (no derivation ancestors)");
            } else {
                for node in &lineage {
                    let indent = "  ".repeat(node.depth as usize);
                    let parent_short =
                        &node.parent_hash[..std::cmp::min(30, node.parent_hash.len())];
                    println!(
                        "{}{} {} {}",
                        indent,
                        "←".dimmed(),
                        parent_short,
                        format!("[{}]", node.transformation_type).dimmed()
                    );
                }
            }
        }
    }

    Ok(())
}
