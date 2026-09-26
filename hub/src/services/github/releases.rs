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
