use std::path::Path;

use cliproot_store::{PackManifest, PackRootMode, Repository};

use crate::OutputFormat;

pub fn create(
    project_id: Option<&str>,
    roots: &[String],
    depth: Option<u32>,
    output: &str,
    format: &OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    let manifest = repo.create_pack(project_id, roots, depth, Path::new(output))?;

    match format {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "path": output,
                    "manifest": manifest
                }))?
            );
        }
        OutputFormat::Text | OutputFormat::Table => {
            println!("Created pack at {output}");
            print_manifest_summary(&manifest);
        }
    }
    Ok(())
}

pub fn import(
    path: &str,
    restore_artifacts: Option<&str>,
    format: &OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    let manifest = repo.import_pack(Path::new(path), restore_artifacts.map(Path::new))?;

    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&manifest)?),
        OutputFormat::Text | OutputFormat::Table => {
            println!("Imported pack from {path}");
            if let Some(dir) = restore_artifacts {
                println!("Restored artifacts to {dir}");
            }
            print_manifest_summary(&manifest);
        }
    }
    Ok(())
}

pub fn inspect(path: &str, format: &OutputFormat) -> Result<(), Box<dyn std::error::Error>> {
    let manifest = Repository::inspect_pack(Path::new(path))?;

    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&manifest)?),
        OutputFormat::Text | OutputFormat::Table => print_manifest_summary(&manifest),
    }
    Ok(())
}

pub fn verify(path: &str, format: &OutputFormat) -> Result<(), Box<dyn std::error::Error>> {
    let manifest = Repository::verify_pack(Path::new(path))?;

    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&manifest)?),
        OutputFormat::Text | OutputFormat::Table => {
            println!("Verified pack {path}");
            print_manifest_summary(&manifest);
        }
    }
    Ok(())
}

fn print_manifest_summary(manifest: &PackManifest) {
    println!("Format: {}", manifest.format);
    println!("Created At: {}", manifest.created_at);
    let mode = match manifest.roots.mode {
        PackRootMode::Project => "project",
        PackRootMode::Roots => "roots",
    };
    println!("Mode: {mode}");
    if let Some(project) = &manifest.project {
        println!("Project: {} ({})", project.name, project.id);
    }
    println!("Roots: {}", manifest.roots.clip_hashes.len());
    for clip_hash in &manifest.roots.clip_hashes {
        println!("  {clip_hash}");
    }
    println!(
        "Counts: bundles={}, clips={}, edges={}, artifacts={}, links={}",
        manifest.counts.bundles,
        manifest.counts.clips,
        manifest.counts.edges,
        manifest.counts.artifacts,
        manifest.counts.links
    );
}
