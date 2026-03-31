use cliproot_store::Repository;

use crate::OutputFormat;

pub fn create(
    id: &str,
    name: &str,
    description: Option<String>,
    format: &OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    let project = repo.create_project(id, name, description)?;
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&project)?),
        _ => println!("Created project {} ({})", project.name, project.id),
    }
    Ok(())
}

pub fn list(format: &OutputFormat) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    let projects = repo.list_projects()?;
    match format {
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&projects)?),
        _ => {
            for project in &projects {
                println!("{}\t{}", project.id, project.name);
            }
            println!("\n{} project(s)", projects.len());
        }
    }
    Ok(())
}

pub fn use_project(project_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    repo.use_project(project_id)?;
    println!("Current project set to {project_id}");
    Ok(())
}

pub fn delete(project_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    repo.delete_project(project_id)?;
    println!("Deleted project {project_id}");
    Ok(())
}
