use cliproot_core::CrpBundle;
use cliproot_store::Repository;

use crate::OutputFormat;

pub fn run(path: &str, format: &OutputFormat) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    let json = std::fs::read_to_string(path)?;
    let bundle: CrpBundle = serde_json::from_str(&json)?;

    let hash = repo.ingest_bundle(&bundle)?;

    match format {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::json!({
                    "status": "ingested",
                    "bundleHash": hash,
                    "clips": bundle.clips.len(),
                })
            );
        }
        _ => {
            println!(
                "Ingested {} clip(s), {} edge(s) from {}",
                bundle.clips.len(),
                bundle.derivation_edges.len(),
                path
            );
        }
    }

    Ok(())
}
