use std::fs;
use std::path::PathBuf;

const SERVICE_NAME: &str = "cliproot-registry";

/// Resolve a token for the given registry URL.
///
/// Resolution order:
/// 1. `CLIPROOT_TOKEN` environment variable (for CI)
/// 2. System keychain
/// 3. Credentials file at `~/.cliproot/credentials.json`
pub fn get_token(registry_url: &str) -> Option<String> {
    // 1. Environment variable
    if let Ok(token) = std::env::var("CLIPROOT_TOKEN") {
        if !token.is_empty() {
            return Some(token);
        }
    }

    // 2. System keychain
    if let Some(token) = get_keychain_token(registry_url) {
        return Some(token);
    }

    // 3. Credentials file
    get_file_token(registry_url)
}

/// Store a token for the given registry URL.
/// Tries system keychain first, falls back to credentials file.
pub fn store_token(registry_url: &str, token: &str) -> Result<(), String> {
    // Try keychain first
    if store_keychain_token(registry_url, token).is_ok() {
        // Also remove any stale file credential
        let _ = delete_file_token(registry_url);
        return Ok(());
    }

    // Fall back to file
    store_file_token(registry_url, token)
}

/// Delete a stored token for the given registry URL.
pub fn delete_token(registry_url: &str) -> Result<(), String> {
    let _ = delete_keychain_token(registry_url);
    let _ = delete_file_token(registry_url);
    Ok(())
}

// --- Keychain helpers ---

fn get_keychain_token(registry_url: &str) -> Option<String> {
    let entry = keyring::Entry::new(SERVICE_NAME, registry_url).ok()?;
    entry.get_password().ok()
}

fn store_keychain_token(registry_url: &str, token: &str) -> Result<(), keyring::Error> {
    let entry = keyring::Entry::new(SERVICE_NAME, registry_url)?;
    entry.set_password(token)
}

fn delete_keychain_token(registry_url: &str) -> Result<(), keyring::Error> {
    let entry = keyring::Entry::new(SERVICE_NAME, registry_url)?;
    entry.delete_credential()
}

// --- File-based credential helpers ---

fn credentials_path() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".cliproot").join("credentials.json"))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .map(PathBuf::from)
}

fn get_file_token(registry_url: &str) -> Option<String> {
    let path = credentials_path()?;
    let data = fs::read_to_string(&path).ok()?;
    let creds: serde_json::Value = serde_json::from_str(&data).ok()?;
    creds
        .get(registry_url)?
        .get("token")?
        .as_str()
        .map(String::from)
}

fn store_file_token(registry_url: &str, token: &str) -> Result<(), String> {
    let path = credentials_path().ok_or("could not determine home directory")?;
    let dir = path.parent().unwrap();
    fs::create_dir_all(dir).map_err(|e| format!("failed to create {}: {e}", dir.display()))?;

    // Read existing credentials or start fresh
    let mut creds: serde_json::Value = if path.exists() {
        let data = fs::read_to_string(&path).map_err(|e| format!("failed to read credentials: {e}"))?;
        serde_json::from_str(&data).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    creds[registry_url] = serde_json::json!({ "token": token });

    let data =
        serde_json::to_string_pretty(&creds).map_err(|e| format!("failed to serialize: {e}"))?;
    fs::write(&path, data).map_err(|e| format!("failed to write credentials: {e}"))?;

    // Restrict file permissions on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o600);
        let _ = fs::set_permissions(&path, perms);
    }

    Ok(())
}

fn delete_file_token(registry_url: &str) -> Result<(), String> {
    let path = match credentials_path() {
        Some(p) if p.exists() => p,
        _ => return Ok(()),
    };

    let data = fs::read_to_string(&path).map_err(|e| format!("failed to read credentials: {e}"))?;
    let mut creds: serde_json::Value =
        serde_json::from_str(&data).unwrap_or_else(|_| serde_json::json!({}));

    if let Some(obj) = creds.as_object_mut() {
        obj.remove(registry_url);
    }

    let data =
        serde_json::to_string_pretty(&creds).map_err(|e| format!("failed to serialize: {e}"))?;
    fs::write(&path, data).map_err(|e| format!("failed to write credentials: {e}"))?;
    Ok(())
}
