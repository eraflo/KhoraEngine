// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! GitHub Releases API client.
//!
//! Fetches available engine versions from
//! `https://api.github.com/repos/eraflo/KhoraEngine/releases`.

use anyhow::{Context, Result};
use serde::Deserialize;

/// Subset of a GitHub release object we care about.
#[derive(Debug, Clone, Deserialize)]
pub struct GithubRelease {
    pub tag_name: String,
    pub name: Option<String>,
    pub body: Option<String>,
    pub prerelease: bool,
    pub assets: Vec<GithubAsset>,
}

/// A downloadable file attached to a release.
#[derive(Debug, Clone, Deserialize)]
pub struct GithubAsset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}

impl GithubRelease {
    /// Returns the engine asset (containing `khora-editor`) for the current OS,
    /// if any.
    ///
    /// Convention: release archives are named `khora-engine-{platform}.{ext}`
    /// where platform is one of `windows-x86_64`, `linux-x86_64`, or
    /// `macos-aarch64`. The hub asset (`khora-hub-*`) is intentionally ignored
    /// — the hub does not download itself.
    pub fn editor_asset(&self) -> Option<&GithubAsset> {
        let needle = if cfg!(windows) {
            "khora-engine-windows"
        } else if cfg!(target_os = "macos") {
            "khora-engine-macos"
        } else {
            "khora-engine-linux"
        };
        self.assets.iter().find(|a| a.name.starts_with(needle))
    }

    /// Returns the runtime asset (containing `khora-runtime`) for the current
    /// OS, if any. Same naming convention as [`Self::editor_asset`] —
    /// `khora-runtime-{platform}.{ext}` produced by `release.yml`. Older
    /// releases predate the runtime artifact and return `None`; the engine
    /// stays usable for editing in that case (the editor's "Build Game"
    /// feature is what needs the runtime).
    pub fn runtime_asset(&self) -> Option<&GithubAsset> {
        let needle = if cfg!(windows) {
            "khora-runtime-windows"
        } else if cfg!(target_os = "macos") {
            "khora-runtime-macos"
        } else {
            "khora-runtime-linux"
        };
        self.assets.iter().find(|a| a.name.starts_with(needle))
    }
}

/// Fetches all releases from the KhoraEngine GitHub repository.
pub fn fetch_releases() -> Result<Vec<GithubRelease>> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("khora-hub/0.1")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .context("Failed to build HTTP client")?;

    let response = client
        .get("https://api.github.com/repos/eraflo/KhoraEngine/releases")
        .send()
        .context("Failed to send request to GitHub API")?;

    if !response.status().is_success() {
        anyhow::bail!("GitHub API returned status {}", response.status());
    }

    let releases: Vec<GithubRelease> = response
        .json()
        .context("Failed to parse GitHub releases JSON")?;

    Ok(releases)
}

/// Spawns a background thread that calls [`fetch_releases`] once and
/// reports the outcome on the returned channel. Used by hub screens
/// that need to refresh in the background without blocking the UI.
pub fn fetch_releases_async() -> std::sync::mpsc::Receiver<Result<Vec<GithubRelease>, String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = fetch_releases().map_err(|e| e.to_string());
        let _ = tx.send(result);
    });
    rx
}

/// The authenticated GitHub user — subset of `GET /user`.
#[derive(Debug, Clone, Deserialize)]
pub struct AuthenticatedUser {
    pub login: String,
    #[serde(default)]
    pub name: Option<String>,
}

/// A GitHub repository — subset of the `POST /user/repos` response we care about.
#[derive(Debug, Clone, Deserialize)]
pub struct CreatedRepo {
    pub full_name: String,
    pub clone_url: String,
    pub ssh_url: String,
    pub html_url: String,
}

fn authed_client(token: &str) -> Result<reqwest::blocking::Client> {
    use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue};

    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {token}"))
            .context("Token contains invalid header characters")?,
    );
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/vnd.github+json"),
    );

    reqwest::blocking::Client::builder()
        .user_agent("khora-hub/0.1")
        .timeout(std::time::Duration::from_secs(15))
        .default_headers(headers)
        .build()
        .context("Failed to build authenticated HTTP client")
}

/// Fetches the authenticated user via `GET /user`.
pub fn get_authenticated_user(token: &str) -> Result<AuthenticatedUser> {
    let client = authed_client(token)?;
    let response = client
        .get("https://api.github.com/user")
        .send()
        .context("Failed to call GitHub /user")?;

    if !response.status().is_success() {
        anyhow::bail!("GitHub /user returned status {}", response.status());
    }

    response.json().context("Failed to parse GitHub user JSON")
}

/// Creates a new repository under the authenticated user via `POST /user/repos`.
pub fn create_repo(token: &str, name: &str, private: bool) -> Result<CreatedRepo> {
    let client = authed_client(token)?;
    let body = serde_json::json!({
        "name": name,
        "private": private,
        "auto_init": false,
    });

    let response = client
        .post("https://api.github.com/user/repos")
        .json(&body)
        .send()
        .context("Failed to call GitHub /user/repos")?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().unwrap_or_default();
        anyhow::bail!("GitHub /user/repos returned {}: {}", status, text);
    }

    response.json().context("Failed to parse created-repo JSON")
}
