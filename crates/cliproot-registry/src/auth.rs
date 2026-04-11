use crate::error::RegistryError;
use crate::types::{DeviceCodeResponse, DeviceTokenError, TokenResponse};

pub struct DeviceFlowClient {
    http: reqwest::blocking::Client,
}

impl DeviceFlowClient {
    pub fn new() -> Result<Self, RegistryError> {
        let http = reqwest::blocking::Client::builder()
            .user_agent(concat!("cliproot/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self { http })
    }

    /// Initiate the device authorization flow.
    /// Returns device code, user code, and verification URI.
    pub fn initiate(
        &self,
        base_url: &str,
        client_id: &str,
    ) -> Result<DeviceCodeResponse, RegistryError> {
        let url = format!("{base_url}/api/auth/device/code");
        let resp = self
            .http
            .post(&url)
            .json(&serde_json::json!({
                "client_id": client_id,
            }))
            .send()?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            return Err(RegistryError::Api {
                code: format!("device_code_{}", status.as_u16()),
                message: format!("Failed to initiate device flow: {body}"),
            });
        }

        Ok(resp.json()?)
    }

    /// Poll the token endpoint until the device is authorized.
    /// Blocks and respects the polling interval.
    /// Returns the access token on success.
    pub fn poll(
        &self,
        base_url: &str,
        device_code: &str,
        client_id: &str,
        interval_secs: u64,
    ) -> Result<TokenResponse, RegistryError> {
        let url = format!("{base_url}/api/auth/device/token");
        let mut interval = std::time::Duration::from_secs(interval_secs);

        loop {
            std::thread::sleep(interval);

            let resp = self
                .http
                .post(&url)
                .json(&serde_json::json!({
                    "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
                    "device_code": device_code,
                    "client_id": client_id,
                }))
                .send()?;

            if resp.status().is_success() {
                return Ok(resp.json()?);
            }

            // Parse the error response
            let error: DeviceTokenError = resp.json().map_err(|e| {
                RegistryError::InvalidRegistry(format!("failed to parse token error: {e}"))
            })?;

            match error.error.as_str() {
                "authorization_pending" => {
                    // Keep polling
                    continue;
                }
                "slow_down" => {
                    // Increase interval by 5 seconds per RFC 8628
                    interval += std::time::Duration::from_secs(5);
                    continue;
                }
                "expired_token" => {
                    return Err(RegistryError::Api {
                        code: "expired_token".into(),
                        message: "Device code expired. Please try again.".into(),
                    });
                }
                "access_denied" => {
                    return Err(RegistryError::Api {
                        code: "access_denied".into(),
                        message: "Authorization was denied by the user.".into(),
                    });
                }
                _ => {
                    return Err(RegistryError::Api {
                        code: error.error,
                        message: error.error_description,
                    });
                }
            }
        }
    }
}
