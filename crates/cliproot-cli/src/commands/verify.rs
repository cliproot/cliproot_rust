use cliproot_store::Repository;
use colored::Colorize;

use crate::OutputFormat;

pub fn run(
    hash_or_id: Option<&str>,
    format: &OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;

    match hash_or_id {
        Some(id) => {
            repo.verify_clip(id)?;
            match format {
                OutputFormat::Json => {
                    println!(
                        "{}",
                        serde_json::json!({"status": "ok", "clipHashOrId": id})
                    );
                }
                _ => {
                    println!("{} {id}", "OK".green().bold());
                }
            }
        }
        None => {
            let errors = repo.verify_all()?;
            let clips = repo.list_clips(None, None, Some(10000))?;
            match format {
                OutputFormat::Json => {
                    println!(
                        "{}",
                        serde_json::json!({
                            "total": clips.len(),
                            "errors": errors,
                        })
                    );
                }
                _ => {
                    if errors.is_empty() {
                        println!(
                            "{} All {} clips verified",
                            "OK".green().bold(),
                            clips.len()
                        );
                    } else {
                        for e in &errors {
                            println!("{} {e}", "FAIL".red().bold());
                        }
                        println!(
                            "{}/{} clips have errors",
                            errors.len(),
                            clips.len()
                        );
                    }
                }
            }
        }
    }

    Ok(())
}
