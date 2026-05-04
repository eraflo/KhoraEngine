// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Hub configuration stored on disk.
//!
//! Located at `~/.khora/hub.json`.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A recently opened or created project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentProject {
    /// Display name of the project.
    pub name: String,
    /// Absolute path to the project directory.
    pub path: String,
    /// Engine version used (e.g. "0.1.0" or "dev").
    pub engine_version: String,
    /// Last opened timestamp as Unix time.
    pub last_opened: u64,
}

/// A downloaded / available engine installation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineInstall {
    /// Version tag (e.g. "v0.1.0").
    pub version: String,
    /// Path to the editor binary.
    pub editor_binary: String,
    /// "github" | "local"
    pub source: String,
}

/// Persistent hub configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HubConfig {
    /// Recently opened projects.
    pub recent_projects: Vec<RecentProject>,
    /// Available engine installations.
    pub engines: Vec<EngineInstall>,
    /// Optional path to the local engine repository (for dev mode).
    pub local_engine_repo: Option<String>,
}

impl HubConfig {
    /// Returns the path to the hub configuration file.
    pub fn config_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".khora")
            .join("hub.json")
    }

    /// Loads the configuration from disk, creating defaults if absent.
    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
                Err(_) => Self::default(),
            }
        } else {
            Self::default()
        }
    }

    /// Saves the configuration to disk.
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, text)?;
        Ok(())
    }

    /// Adds or updates a recent project entry, keeping the list sorted by
    /// most-recently-opened first.
    pub fn push_recent(&mut self, name: &str, path: &Path, engine_version: &str) {
        let path_str = path.to_string_lossy().to_string();
        self.recent_projects.retain(|p| p.path != path_str);
        self.recent_projects.insert(
            0,
            RecentProject {
                name: name.to_owned(),
                path: path_str,
                engine_version: engine_version.to_owned(),
                last_opened: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            },
        );
        self.recent_projects.truncate(20);
    }

    /// Returns the dev-mode engine install if the local repo is configured.
    pub fn dev_engine(&self) -> Option<EngineInstall> {
        let repo = self.local_engine_repo.as_deref()?;
        // The editor binary is expected at <repo>/target/debug/khora-editor(.exe)
        let exe = if cfg!(windows) {
            "khora-editor.exe"
        } else {
            "khora-editor"
        };
        let binary = PathBuf::from(repo).join("target").join("debug").join(exe);
        Some(EngineInstall {
            version: "dev".to_owned(),
            editor_binary: binary.to_string_lossy().to_string(),
            source: "local".to_owned(),
        })
    }
}
