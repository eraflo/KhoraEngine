// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! New Project form state + the `EngineChoice` that drives the engine
//! combo.

use crate::config::EngineInstall;
use crate::download;
use crate::github;
use std::sync::mpsc;

/// One row of the New Project engine combo.
#[derive(Debug, Clone)]
pub enum EngineChoice {
    /// Already-installed engine (or the dev one).
    Installed(EngineInstall),
    /// Remote release that would be downloaded on demand.
    Remote {
        version: String,
        download_url: String,
        size: u64,
        runtime_url: Option<String>,
        runtime_size: Option<u64>,
    },
}

impl EngineChoice {
    pub fn version(&self) -> &str {
        match self {
            Self::Installed(e) => &e.version,
            Self::Remote { version, .. } => version,
        }
    }
}

/// New Project form state.
pub struct NewProjectState {
    pub name: String,
    pub path: String,
    pub engine_idx: usize,
    pub status: Option<String>,
    pub success: bool,

    pub git_init: bool,
    pub git_remote: bool,
    pub remote_repo_name: String,
    pub remote_private: bool,
    pub remote_push: bool,

    pub releases: Vec<github::GithubRelease>,
    pub fetch_rx: Option<mpsc::Receiver<Result<Vec<github::GithubRelease>, String>>>,
    pub has_fetched_once: bool,

    pub download_rx: Option<mpsc::Receiver<download::DownloadMessage>>,
    pub download_progress: Option<(u64, u64)>,
    pub creating_after_download: bool,
}

impl NewProjectState {
    pub fn new() -> Self {
        let path = dirs::home_dir()
            .map(|h| h.join("KhoraProjects").to_string_lossy().to_string())
            .unwrap_or_default();
        Self {
            name: String::new(),
            path,
            engine_idx: 0,
            status: None,
            success: false,
            git_init: true,
            git_remote: false,
            remote_repo_name: String::new(),
            remote_private: true,
            remote_push: true,
            releases: Vec::new(),
            fetch_rx: None,
            has_fetched_once: false,
            download_rx: None,
            download_progress: None,
            creating_after_download: false,
        }
    }
}

impl Default for NewProjectState {
    fn default() -> Self {
        Self::new()
    }
}
