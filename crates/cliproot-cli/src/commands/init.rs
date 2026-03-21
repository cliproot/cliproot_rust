use cliproot_store::Repository;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    Repository::init(&cwd)?;
    println!("Initialized .cliproot/ repository in {}", cwd.display());
    Ok(())
}
