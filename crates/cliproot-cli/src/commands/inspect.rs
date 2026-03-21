use cliproot_store::Repository;

use crate::output::print_clip;
use crate::OutputFormat;

pub fn run(hash_or_id: &str, format: &OutputFormat) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    let clip = repo
        .get_clip(hash_or_id)?
        .ok_or_else(|| format!("clip not found: {hash_or_id}"))?;
    print_clip(&clip, format);
    Ok(())
}
