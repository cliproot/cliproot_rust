use cliproot_core::matching::parse_annotation_style;
use cliproot_store::Repository;

use crate::OutputFormat;

pub fn run(
    file: &str,
    style: &str,
    in_place: bool,
    threshold: f64,
    format: &OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    let document_text = std::fs::read_to_string(file)?;
    let style = parse_annotation_style(style).map_err(|e| format!("invalid --style: {e}"))?;

    let result = repo.annotate(&document_text, style, threshold)?;

    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        _ => {
            if in_place {
                std::fs::write(file, &result.annotated_text)?;
                eprintln!(
                    "Annotated {} with {} citation(s)",
                    file,
                    result.citations.len()
                );
            } else {
                print!("{}", result.annotated_text);
            }
        }
    }

    Ok(())
}
