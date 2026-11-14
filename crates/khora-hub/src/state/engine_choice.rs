// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! `EngineChoice`: one row of the New Project engine combo.

use crate::services::config::EngineInstall;

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
