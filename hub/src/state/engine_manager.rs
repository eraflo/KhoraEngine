// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Engine Manager screen state.

use crate::download;
use crate::github;
use std::sync::mpsc;

pub struct EngineManagerState {
    pub local_repo: String,
    pub releases: Vec<github::GithubRelease>,
    pub fetch_error: Option<String>,
    pub fetching: bool,
    pub has_fetched_once: bool,
    pub download_rx: Option<mpsc::Receiver<download::DownloadMessage>>,
    pub download_progress: Option<(u64, u64)>,
    pub fetch_rx: Option<mpsc::Receiver<Result<Vec<github::GithubRelease>, String>>>,
}

impl EngineManagerState {
    pub fn new(local_repo: String) -> Self {
        Self {
            local_repo,
            releases: Vec::new(),
            fetch_error: None,
            fetching: false,
            has_fetched_once: false,
            download_rx: None,
            download_progress: None,
            fetch_rx: None,
        }
    }
}
