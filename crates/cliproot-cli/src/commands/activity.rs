use cliproot_core::ActivityType;
use cliproot_store::Repository;

use crate::OutputFormat;

pub fn start(
    activity_type: &str,
    prompt: Option<String>,
    agent: Option<&str>,
    project_id: Option<&str>,
    parameters: Option<&str>,
    session_id: Option<&str>,
    format: &OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    let activity_type: ActivityType =
        serde_json::from_value(serde_json::Value::String(activity_type.to_string()))?;
    let parameters = parameters.map(serde_json::from_str).transpose()?;
    let activity = repo.start_activity(
        activity_type,
        project_id,
        agent,
        prompt,
        parameters,
        session_id,
    )?;

    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&activity)?),
        _ => println!("Started activity {}", activity.id),
    }

    Ok(())
}

pub fn end(activity_id: &str, format: &OutputFormat) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    let activity = repo.end_activity(activity_id)?;
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&activity)?),
        _ => println!("Ended activity {}", activity.id),
    }
    Ok(())
}
