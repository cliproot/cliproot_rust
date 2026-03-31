use cliproot_store::Repository;

use crate::OutputFormat;

pub fn start(
    agent: Option<&str>,
    project_id: Option<&str>,
    metadata: Option<&str>,
    format: &OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    let metadata = metadata.map(serde_json::from_str).transpose()?;
    let session = repo.start_session(project_id, agent, metadata)?;
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&session)?),
        _ => println!("Started session {}", session.session_id),
    }
    Ok(())
}

pub fn end(session_id: &str, format: &OutputFormat) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    let session = repo.end_session(session_id)?;
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&session)?),
        _ => println!(
            "Ended session {} ({})",
            session.session_id,
            session.artifact_hash.as_deref().unwrap_or("no artifact")
        ),
    }
    Ok(())
}
