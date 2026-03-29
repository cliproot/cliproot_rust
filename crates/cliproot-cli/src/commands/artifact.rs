use std::path::Path;

use cliproot_core::{ArtifactType, ClipArtifactRelationship};
use cliproot_store::Repository;

use crate::OutputFormat;

pub fn add(
    path: Option<&str>,
    content: Option<&str>,
    file_name: Option<&str>,
    artifact_type: &str,
    mime_type: Option<&str>,
    id: Option<&str>,
    project_id: Option<&str>,
    format: &OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    let artifact_type: ArtifactType =
        serde_json::from_value(serde_json::Value::String(artifact_type.to_string()))?;
    let artifact = repo.add_artifact(
        path.map(Path::new),
        content.map(str::as_bytes),
        file_name,
        artifact_type,
        mime_type,
        id,
        project_id,
        None,
    )?;

    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&artifact)?),
        _ => println!("Stored artifact {} ({})", artifact.file_name, artifact.artifact_hash),
    }
    Ok(())
}

pub fn list(
    project_id: Option<&str>,
    format: &OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    let artifacts = repo.list_artifacts(project_id)?;
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&artifacts)?),
        _ => {
            for artifact in &artifacts {
                let kind = serde_json::to_value(&artifact.artifact_type)?
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string();
                println!("{}\t{}\t{}", artifact.artifact_hash, kind, artifact.file_name);
            }
            println!("\n{} artifact(s)", artifacts.len());
        }
    }
    Ok(())
}

pub fn get(
    artifact_hash: &str,
    format: &OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    let artifact = repo
        .get_artifact(artifact_hash)?
        .ok_or_else(|| format!("artifact not found: {artifact_hash}"))?;
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&artifact)?),
        _ => println!("{}\t{}\t{}", artifact.artifact_hash, artifact.mime_type, artifact.file_name),
    }
    Ok(())
}

pub fn restore(
    artifact_hash: &str,
    output: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    let path = repo.restore_artifact(artifact_hash, output.map(Path::new))?;
    println!("Restored artifact to {}", path.display());
    Ok(())
}

pub fn link(
    clip_hash_or_id: &str,
    artifact_hash: &str,
    relationship: &str,
    format: &OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    let relationship: ClipArtifactRelationship =
        serde_json::from_value(serde_json::Value::String(relationship.to_string()))?;
    let link = repo.link_clip_artifact(clip_hash_or_id, artifact_hash, relationship)?;
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&link)?),
        _ => println!(
            "Linked clip {} -> artifact {}",
            link.clip_hash, link.artifact_hash
        ),
    }
    Ok(())
}
