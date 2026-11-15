// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! GitHub sign-in through the OAuth device flow, and the token kept on disk.
//!
//! See <https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps#device-flow>.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

/// Client ID of the Khora Hub OAuth app, embedded at build time; `None` in a build without it.
///
/// Only the release pipeline (`release.yml` on `main`) holds the
/// `KHORA_HUB_GITHUB_CLIENT_ID` secret, so only release binaries can sign in.
/// There is deliberately no fallback in the source.
pub const KHORA_HUB_CLIENT_ID: Option<&str> = option_env!("KHORA_HUB_GITHUB_CLIENT_ID");

/// OAuth scope: full repo access (private + public) so the hub can create
/// private repos for projects.
const SCOPES: &str = "repo";

// ── Device flow types ────────────────────────────────────────────

/// GitHub's answer to a device-code request.
#[derive(Debug, Clone, Deserialize)]
pub struct DeviceCode {
    /// The code the hub polls the token endpoint with.
    pub device_code: String,
    /// The code the user types on the verification page.
    pub user_code: String,
    /// The page where the user types `user_code`.
    pub verification_uri: String,
    /// Seconds before the codes expire.
    #[serde(default = "default_expires_in")]
    pub expires_in: u64,
    /// Seconds to wait between two polls.
    #[serde(default = "default_interval")]
    pub interval: u64,
}

fn default_expires_in() -> u64 {
    900
}
fn default_interval() -> u64 {
    5
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    error: Option<String>,
    interval: Option<u64>,
}

/// Result of polling the device-flow token endpoint.
#[derive(Debug)]
pub enum AuthMessage {
    /// Show the user code + verification URI to the user.
    DeviceCodeReady(DeviceCode),
    /// Authentication completed; token has been stored on disk.
    Authenticated { token: String, login: String },
    /// User denied or the request expired.
    Failed(String),
}

// ── Public API ───────────────────────────────────────────────────

/// Starts the device flow in the background and returns the channel of its messages.
///
/// It sends `DeviceCodeReady` once GitHub gives a code, then `Authenticated`
/// (the token stored on disk, or a warning logged when it cannot be), or `Failed`.
pub fn start_device_flow() -> mpsc::Receiver<AuthMessage> {
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let device = match request_device_code() {
            Ok(d) => d,
            Err(e) => {
                let _ = tx.send(AuthMessage::Failed(format!(
                    "Failed to request device code: {e}"
                )));
                return;
            }
        };

        let _ = tx.send(AuthMessage::DeviceCodeReady(device.clone()));

        match poll_token(&device) {
            Ok(token) => match crate::services::github::get_authenticated_user(&token) {
                Ok(user) => {
                    if let Err(e) = store_token(&token) {
                        log::warn!("Failed to persist token: {e}");
                    }
                    let _ = tx.send(AuthMessage::Authenticated {
                        token,
                        login: user.login,
                    });
                }
                Err(e) => {
                    let _ = tx.send(AuthMessage::Failed(format!(
                        "Token obtained but /user failed: {e}"
                    )));
                }
            },
            Err(e) => {
                let _ = tx.send(AuthMessage::Failed(format!("Authorization failed: {e}")));
            }
        }
    });

    rx
}

/// Returns the path of the credentials file, `~/.khora/credentials.json`.
///
/// It is kept apart from `hub.json` so that `HubConfig` holds no secret.
pub fn credentials_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".khora")
        .join("credentials.json")
}

#[derive(Debug, serde::Serialize, Deserialize)]
struct StoredCredentials {
    github_token: String,
}

/// Reads the persisted token, if any.
pub fn load_token() -> Option<String> {
    let path = credentials_path();
    let text = std::fs::read_to_string(&path).ok()?;
    let creds: StoredCredentials = serde_json::from_str(&text).ok()?;
    Some(creds.github_token)
}

/// Writes `token` to the credentials file, readable by its owner only on Unix.
///
/// # Errors
///
/// Fails when the file or its folder cannot be written.
pub fn store_token(token: &str) -> Result<()> {
    let path = credentials_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let creds = StoredCredentials {
        github_token: token.to_owned(),
    };
    let text = serde_json::to_string_pretty(&creds)?;
    std::fs::write(&path, text).context("Failed to write credentials.json")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path)?.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(&path, perms)?;
    }

    Ok(())
}

/// Removes the credentials file; a missing one is no error.
///
/// # Errors
///
/// Fails when the file exists but cannot be removed.
pub fn forget_token() -> Result<()> {
    let path = credentials_path();
    if path.exists() {
        std::fs::remove_file(&path).context("Failed to remove credentials.json")?;
    }
    Ok(())
}

// ── Internal: device-code endpoints ──────────────────────────────

fn http_client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .user_agent("khora-hub/0.1")
        .timeout(Duration::from_secs(15))
        .build()
        .context("Failed to build HTTP client")
}

/// Returns the build-time Client ID, or an error pointing to the official release binaries.
fn require_client_id() -> Result<&'static str> {
    KHORA_HUB_CLIENT_ID.ok_or_else(|| {
        anyhow::anyhow!(
            "This Khora Hub build was compiled without a GitHub OAuth Client ID, \
             so GitHub login is disabled. Use an official release binary from \
             https://github.com/eraflo/KhoraEngine/releases to sign in — only \
             release builds embed the project's OAuth credentials."
        )
    })
}

fn request_device_code() -> Result<DeviceCode> {
    let client_id = require_client_id()?;
    let client = http_client()?;
    let response = client
        .post("https://github.com/login/device/code")
        .header(reqwest::header::ACCEPT, "application/json")
        .form(&[("client_id", client_id), ("scope", SCOPES)])
        .send()
        .context("Failed to POST /login/device/code")?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().unwrap_or_default();
        anyhow::bail!("/login/device/code returned {status}: {text}");
    }

    response
        .json::<DeviceCode>()
        .context("Failed to parse DeviceCode JSON")
}

fn poll_token(device: &DeviceCode) -> Result<String> {
    let client_id = require_client_id()?;
    let client = http_client()?;
    let mut interval = device.interval.max(1);
    let deadline = std::time::Instant::now() + Duration::from_secs(device.expires_in);

    loop {
        if std::time::Instant::now() > deadline {
            anyhow::bail!("Device code expired before user authorized");
        }
        std::thread::sleep(Duration::from_secs(interval));

        let response = client
            .post("https://github.com/login/oauth/access_token")
            .header(reqwest::header::ACCEPT, "application/json")
            .form(&[
                ("client_id", client_id),
                ("device_code", device.device_code.as_str()),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .context("Failed to POST /login/oauth/access_token")?;

        let body: TokenResponse = response.json().context("Failed to parse token JSON")?;

        if let Some(token) = body.access_token {
            return Ok(token);
        }

        match body.error.as_deref() {
            Some("authorization_pending") => {
                // keep polling
            }
            Some("slow_down") => {
                interval = body.interval.unwrap_or(interval).max(interval + 5);
            }
            Some("expired_token") => anyhow::bail!("Device code expired"),
            Some("access_denied") => anyhow::bail!("User denied the authorization"),
            Some(other) => anyhow::bail!("OAuth error: {other}"),
            None => anyhow::bail!("Unexpected empty response from token endpoint"),
        }
    }
}
