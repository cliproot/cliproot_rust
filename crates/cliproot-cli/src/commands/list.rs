use cliproot_store::Repository;

use crate::output::print_clip_row;
use crate::OutputFormat;

pub fn run(
    document: Option<&str>,
    source_type: Option<&str>,
    limit: u32,
    format: &OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    let clips = repo.list_clips(document, source_type, Some(limit))?;

    if matches!(format, OutputFormat::Table) {
        println!("{:<22} {:<16} CONTENT", "HASH", "ID");
        println!("{}", "-".repeat(80));
    }

    for clip in &clips {
        print_clip_row(clip, format);
    }

    if !matches!(format, OutputFormat::Json) {
        println!("\n{} clip(s)", clips.len());
    }

    Ok(())
}
