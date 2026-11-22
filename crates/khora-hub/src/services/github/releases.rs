// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! GitHub Releases API client.
//!
//! Fetches the engine versions from
//! `https://api.github.com/repos/eraflo/KhoraEngine/releases`. The repository
//! also publishes releases that are not engines (the docs site's frame
//! archive, a hub-only build): an engine version is a release carrying the
//! engine archive for this platform (`GithubRelease::editor_asset`), and no
//! other release leaves this module.

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
    /// Returns the engine archive, holding `khora-editor`, for this machine.
    ///
    /// Release archives are named `khora-engine-{platform}.{ext}` (see
    /// [`archive_name`]); only that exact name is taken, so a checksum or an
    /// archive for another architecture beside it is never offered. The hub's
    /// own archive (`khora-hub-*`) is never offered: the hub does not download itself.
    pub(crate) fn editor_asset(&self) -> Option<&GithubAsset> {
        self.asset_named(&archive_name("engine")?)
    }

    /// Returns the runtime archive, holding `khora-runtime`, for this machine.
    ///
    /// It is named `khora-runtime-{platform}.{ext}`, as for
    /// [`Self::editor_asset`]. A release without one still edits; only the
    /// editor's Build Game needs the runtime.
    pub(crate) fn runtime_asset(&self) -> Option<&GithubAsset> {
        self.asset_named(&archive_name("runtime")?)
    }

    /// Returns the file attached under exactly `name`.
    fn asset_named(&self, name: &str) -> Option<&GithubAsset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

/// Returns whether releases carry an engine archive for this machine's OS
/// and architecture; where they don't, no release is ever an engine version.
pub fn builds_for_this_machine() -> bool {
    archive_name("engine").is_some()
}

/// Returns the name of the `kind` archive (`engine`, `runtime`) a release
/// carries for this machine, or `None` where releases build none.
///
/// The names are the release workflow's (`.github/workflows/release.yml`):
/// `khora-{kind}-windows-x86_64.zip`, `khora-{kind}-linux-x86_64.tar.gz`,
/// `khora-{kind}-macos-aarch64.tar.gz`.
fn archive_name(kind: &str) -> Option<String> {
    let (platform, extension) = if cfg!(all(windows, target_arch = "x86_64")) {
        ("windows-x86_64", "zip")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        ("linux-x86_64", "tar.gz")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        ("macos-aarch64", "tar.gz")
    } else {
        return None;
    };
    Some(format!("khora-{kind}-{platform}.{extension}"))
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

/// Fetches the engine versions of the `KhoraEngine` GitHub repository, newest first.
///
/// A release is kept only when it carries the engine archive for this
/// platform, as [`parse_engine_versions`] says.
///
/// # Errors
///
/// Fails when GitHub cannot be reached, answers with an error status, sends a
/// body that cannot be read, or sends JSON that is not a list of releases.
pub(crate) fn fetch_engine_versions() -> Result<Vec<GithubRelease>> {
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

    let body = response
        .text()
        .context("Failed to read the GitHub releases response")?;
    parse_engine_versions(&body)
}

/// Parses the GitHub releases JSON and keeps the engine versions, in GitHub's order.
///
/// # Errors
///
/// Fails when the JSON is not a list of releases.
pub(crate) fn parse_engine_versions(json: &str) -> Result<Vec<GithubRelease>> {
    let mut releases: Vec<GithubRelease> =
        serde_json::from_str(json).context("Failed to parse GitHub releases JSON")?;
    releases.retain(|r| r.editor_asset().is_some());
    Ok(releases)
}

/// Runs [`fetch_engine_versions`] in the background and returns the channel of its outcome.
#[expect(
    clippy::disallowed_methods,
    reason = "the hub runs no engine: a network call runs on its own thread"
)]
pub(crate) fn fetch_engine_versions_async()
-> std::sync::mpsc::Receiver<Result<Vec<GithubRelease>, String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = fetch_engine_versions().map_err(|e| e.to_string());
        let _ = tx.send(result);
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::parse_engine_versions;

    /// Returns the name of this platform's engine archive.
    fn engine_archive_here() -> &'static str {
        if cfg!(windows) {
            "khora-engine-windows-x86_64.zip"
        } else if cfg!(target_os = "macos") {
            "khora-engine-macos-aarch64.tar.gz"
        } else {
            "khora-engine-linux-x86_64.tar.gz"
        }
    }

    /// Returns the name of an engine archive built for another platform.
    fn engine_archive_elsewhere() -> &'static str {
        if cfg!(windows) {
            "khora-engine-linux-x86_64.tar.gz"
        } else {
            "khora-engine-windows-x86_64.zip"
        }
    }

    /// Returns the name of this platform's runtime archive.
    fn runtime_archive_here() -> String {
        engine_archive_here().replace("khora-engine-", "khora-runtime-")
    }

    /// Returns one GitHub release object, as the API sends it, with these assets.
    fn release_json(tag: &str, assets: &[&str]) -> String {
        let assets: Vec<String> = assets
            .iter()
            .map(|name| {
                format!(
                    r#"{{"id":1,"name":"{name}","content_type":"application/octet-stream","size":1000,"browser_download_url":"https://example.invalid/{tag}/{name}"}}"#
                )
            })
            .collect();
        format!(
            r#"{{"id":1,"tag_name":"{tag}","name":"{tag}","draft":false,"prerelease":false,"assets":[{}]}}"#,
            assets.join(",")
        )
    }

    /// Returns the tags of the engine versions parsed from these releases.
    fn engine_tags(releases: &[String]) -> Vec<String> {
        let json = format!("[{}]", releases.join(","));
        parse_engine_versions(&json)
            .expect("the fixture parses")
            .into_iter()
            .map(|r| r.tag_name)
            .collect()
    }

    #[test]
    fn a_release_without_this_platforms_engine_archive_is_not_an_engine_version() {
        let tags = engine_tags(&[
            release_json(
                "v0.3.0",
                &[
                    engine_archive_here(),
                    engine_archive_elsewhere(),
                    "khora-runtime-windows-x86_64.zip",
                    "khora-hub-windows-x86_64.zip",
                ],
            ),
            release_json("landing-frames-v1", &["frames.tar.zst"]),
            release_json(
                "hub-v0.2.1",
                &[
                    "khora-hub-windows-x86_64.zip",
                    "khora-hub-linux-x86_64.tar.gz",
                ],
            ),
            release_json("v0.2.0-other-os", &[engine_archive_elsewhere()]),
            release_json("empty", &[]),
        ]);
        assert_eq!(tags, ["v0.3.0"]);
    }

    #[test]
    fn engine_versions_keep_githubs_order() {
        let tags = engine_tags(&[
            release_json("v0.4.0", &[engine_archive_here()]),
            release_json("landing-frames-v1", &["frames.tar.zst"]),
            release_json("v0.3.0", &[engine_archive_here()]),
            release_json("hub-v0.2.1", &["khora-hub-windows-x86_64.zip"]),
            release_json("v0.2.0", &[engine_archive_here()]),
        ]);
        assert_eq!(tags, ["v0.4.0", "v0.3.0", "v0.2.0"]);
    }

    #[test]
    fn a_file_named_like_the_engine_archive_but_longer_is_not_an_engine_version() {
        let checksum = format!("{}.sha256", engine_archive_here());
        let tags = engine_tags(&[
            release_json("v0.4.0-checksums", &[checksum.as_str()]),
            release_json("v0.3.0", &[engine_archive_here()]),
        ]);
        assert_eq!(tags, ["v0.3.0"]);
    }

    #[test]
    fn a_body_that_is_not_a_release_list_is_an_error() {
        assert!(parse_engine_versions(r#"{"message":"API rate limit exceeded"}"#).is_err());
    }

    /// **The runtime archive is taken by its exact name**, never a checksum
    /// or another platform's beside it; a release without one has none.
    #[test]
    fn the_runtime_archive_is_taken_by_its_exact_name() {
        let runtime = runtime_archive_here();
        let checksum = format!("{runtime}.sha256");
        let elsewhere = engine_archive_elsewhere().replace("khora-engine-", "khora-runtime-");
        let json = format!(
            "[{},{}]",
            release_json(
                "v0.4.0",
                &[
                    engine_archive_here(),
                    checksum.as_str(),
                    elsewhere.as_str(),
                    runtime.as_str(),
                ],
            ),
            release_json("v0.3.0", &[engine_archive_here(), checksum.as_str()]),
        );
        let releases = parse_engine_versions(&json).expect("the fixture parses");
        assert_eq!(
            releases[0].runtime_asset().map(|a| a.name.as_str()),
            Some(runtime.as_str())
        );
        assert!(
            releases[1].runtime_asset().is_none(),
            "a checksum alone is no runtime archive"
        );
    }

    /// **This machine has engine builds exactly where the release workflow makes them.**
    #[test]
    fn this_machine_has_engine_builds_where_the_release_workflow_makes_them() {
        let made_here = cfg!(any(
            all(windows, target_arch = "x86_64"),
            all(target_os = "linux", target_arch = "x86_64"),
            all(target_os = "macos", target_arch = "aarch64")
        ));
        assert_eq!(super::builds_for_this_machine(), made_here);
    }
}
