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
    /// The release's tag, used as the engine version.
    pub tag_name: String,
    /// Whether GitHub marks it as a pre-release.
    pub prerelease: bool,
    /// The files attached to it.
    pub assets: Vec<GithubAsset>,
}

impl GithubRelease {
    /// Returns the engine archive, holding `khora-editor`, for the current OS.
    ///
    /// Release archives are named `khora-engine-{platform}.{ext}`, the platform
    /// being `windows-x86_64`, `linux-x86_64` or `macos-aarch64`. The hub's own
    /// archive (`khora-hub-*`) is never offered: the hub does not download itself.
    pub(crate) fn editor_asset(&self) -> Option<&GithubAsset> {
        let needle = if cfg!(windows) {
            "khora-engine-windows"
        } else if cfg!(target_os = "macos") {
            "khora-engine-macos"
        } else {
            "khora-engine-linux"
        };
        self.assets.iter().find(|a| a.name.starts_with(needle))
    }

    /// Returns the runtime archive, holding `khora-runtime`, for the current OS.
    ///
    /// It is named `khora-runtime-{platform}.{ext}`, as for
    /// [`Self::editor_asset`]. A release without one still edits; only the
    /// editor's Build Game needs the runtime.
    pub(crate) fn runtime_asset(&self) -> Option<&GithubAsset> {
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

/// A downloadable file attached to a release.
#[derive(Debug, Clone, Deserialize)]
pub struct GithubAsset {
    /// The file name.
    pub name: String,
    /// The URL a plain download fetches it from.
    pub browser_download_url: String,
    /// Its size, in bytes.
    pub size: u64,
}

/// Fetches all releases from the `KhoraEngine` GitHub repository.
///
/// # Errors
///
/// Fails when GitHub cannot be reached, answers with an error status or
/// sends JSON that does not parse.
pub(crate) fn fetch_releases() -> Result<Vec<GithubRelease>> {
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

/// Runs [`fetch_releases`] in the background and returns the channel of its outcome.
#[expect(
    clippy::disallowed_methods,
    reason = "the hub runs no engine: a network call runs on its own thread"
)]
pub(crate) fn fetch_releases_async() -> std::sync::mpsc::Receiver<Result<Vec<GithubRelease>, String>>
{
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = fetch_releases().map_err(|e| e.to_string());
        let _ = tx.send(result);
    });
    rx
}
