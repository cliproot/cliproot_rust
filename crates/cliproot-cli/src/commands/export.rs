use cliproot_store::Repository;

use crate::OutputFormat;

pub fn run(
    hash: &str,
    output: Option<&str>,
    format: &OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    let bundle = repo.export_bundle(hash)?;
    let json = serde_json::to_string_pretty(&bundle)?;

    match output {
        Some(path) => {
            std::fs::write(path, &json)?;
            match format {
                OutputFormat::Json => {
                    println!(
                        "{}",
                        serde_json::json!({
                            "status": "exported",
                            "path": path,
                            "clips": bundle.clips.len(),
                        })
                    );
                }
                _ => {
                    println!(
                        "Exported {} clip(s) to {}",
                        bundle.clips.len(),
                        path
                    );
                }
            }
        }
        None => {
            println!("{json}");
        }
    }

    Ok(())
}
