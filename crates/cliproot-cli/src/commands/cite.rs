use cliproot_store::Repository;

use crate::OutputFormat;

pub fn run(
    file: &str,
    threshold: f64,
    format: &OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    let document_text = std::fs::read_to_string(file)?;

    let citations = repo.cite(&document_text, threshold)?;

    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&citations)?);
        }
        _ => {
            if citations.is_empty() {
                println!("No matching sources found.");
            } else {
                println!("Citations\n");
                for c in &citations {
                    let title = c.source_title.as_deref().unwrap_or("Untitled");
                    let url = c.source_url.as_deref().unwrap_or("(no URL)");
                    println!("[{}] {} — {}", c.index, title, url);
                }
            }
        }
    }

    Ok(())
}
