use std::path::PathBuf;

pub fn run(path: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let path = path.map(PathBuf::from);
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(cliproot_mcp::run_server(path.as_deref()))
}
