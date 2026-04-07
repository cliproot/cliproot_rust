use cliproot_registry::auth::DeviceFlowClient;
use cliproot_registry::credential;
use cliproot_store::Repository;

use crate::OutputFormat;

pub fn run(
    token: Option<&str>,
    remote: Option<&str>,
    format: &OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::discover()?;
    let (_remote_name, remote_config) = super::remote::resolve_remote(&repo, remote)?;
    let registry_url = &remote_config.url;

    // If --token is provided, store it directly (for CI setup).
    if let Some(t) = token {
        credential::store_token(registry_url, t)
            .map_err(|e| format!("failed to store token: {e}"))?;
        match format {
            OutputFormat::Json => println!(r#"{{"status":"ok","message":"Token stored"}}"#),
            _ => println!("Token stored for {registry_url}"),
        }
        return Ok(());
    }

    // Interactive device flow
    let client = DeviceFlowClient::new()?;
    let client_id = "cliproot-cli";

    let device = client.initiate(registry_url, client_id)?;

    match format {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "status": "pending",
                    "user_code": device.user_code,
                    "verification_uri": device.verification_uri,
                    "verification_uri_complete": device.verification_uri_complete,
                    "expires_in": device.expires_in,
                }))?
            );
        }
        _ => {
            println!();
            println!(
                "  Visit: {}",
                device.verification_uri
            );
            println!(
                "  Enter code: {}",
                device.user_code
            );
            println!();
        }
    }

    // Try to open the browser
    let _ = open::that(&device.verification_uri_complete);

    if !matches!(format, OutputFormat::Json) {
        println!("Waiting for authorization...");
    }

    let token_resp = client.poll(
        registry_url,
        &device.device_code,
        client_id,
        device.interval,
    )?;

    credential::store_token(registry_url, &token_resp.access_token)
        .map_err(|e| format!("failed to store token: {e}"))?;

    match format {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "status": "ok",
                    "message": "Logged in",
                    "expires_in": token_resp.expires_in,
                }))?
            );
        }
        _ => {
            println!("Logged in to {registry_url}");
        }
    }

    Ok(())
}
