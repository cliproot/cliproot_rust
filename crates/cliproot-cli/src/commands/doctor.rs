use cliproot_core::matching::CoverageStatus;
use cliproot_store::Repository;
use colored::Colorize;

use crate::OutputFormat;

pub fn run(
    file: &str,
    threshold: f64,
    format: &OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    let document_text = std::fs::read_to_string(file)?;

    let report = repo.doctor(&document_text, threshold)?;

    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        _ => {
            println!("Provenance Coverage Report");
            println!("==========================\n");
            println!(
                "Paragraphs: {}/{} covered",
                report.covered_paragraphs, report.total_paragraphs
            );
            println!();

            for p in &report.paragraph_reports {
                let (icon, label) = match p.status {
                    CoverageStatus::Covered => ("✓".green().to_string(), "covered".to_string()),
                    CoverageStatus::Uncovered => {
                        ("✗".red().to_string(), "missing source".to_string())
                    }
                };
                println!("{} P{}: {} [{}]", icon, p.index + 1, p.text_preview, label);
            }
        }
    }

    Ok(())
}
