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
    /// Returns the editor binary asset for the current OS, if any.
    pub fn editor_asset(&self) -> Option<&GithubAsset> {
        // Convention: assets named khora-editor-windows.zip, khora-editor-linux.tar.gz, etc.
        let suffix = if cfg!(windows) {
            "windows"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else {
            "linux"
        };
        self.assets.iter().find(|a| a.name.contains(suffix))
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
