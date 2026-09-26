// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Opening the project named on the command line.

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::AssetEntry;
use khora_sdk::GameWorld;

use super::EditorApp;
use crate::bootstrap::PROJECT_PATH;
use crate::project_vfs::ProjectVfs;
use crate::{hot_reload, scene_io, util};

impl EditorApp {
    pub(super) fn open_cli_project(&mut self, world: &mut GameWorld) {
        let Some(Some(project_path)) = PROJECT_PATH.get() else {
            return;
        };
        let path = std::path::PathBuf::from(project_path);
        if !path.exists() {
            log::warn!("--project path does not exist: {}", project_path);
            return;
        }

        // Build the project-scoped VFS. This recursively scans the
        // assets directory, builds a stable in-memory UUID index,
        // registers all decoders, and arms the filesystem watcher.
        let metrics = std::sync::Arc::new(khora_sdk::MetricsRegistry::new());
        let mut pvfs = match ProjectVfs::open(path.clone(), metrics) {
            Ok(p) => p,
            Err(e) => {
                log::error!(
                    "Failed to open ProjectVfs for '{}': {:#}",
                    path.display(),
                    e
                );
                return;
            }
        };

        // Read project metadata via the VFS helper instead of a raw
        // `std::fs::read_to_string`.
        let project_json: Option<serde_json::Value> = pvfs.read_project_json().ok();
        let project_name = project_json
            .as_ref()
            .and_then(|v| v.get("name").and_then(|n| n.as_str()).map(String::from));
        let project_engine_version = project_json.as_ref().and_then(|v| {
            v.get("engine_version")
                .and_then(|n| n.as_str())
                .map(String::from)
        });

        let entries = hot_reload::collect_asset_entries(&pvfs);
        let dirs = pvfs.list_dirs();
        let git_branch = util::read_git_branch(&path);

        if let Ok(mut state) = self.editor_state.lock() {
            state.project_folder = Some(path.to_string_lossy().to_string());
            state.project_name = project_name.clone();
            state.project_engine_version = project_engine_version.clone();
            state.asset_entries = entries;
            state.asset_dirs = dirs;
            state.asset_epoch = state.asset_epoch.wrapping_add(1);
            state.current_git_branch = git_branch.clone();
            log::info!(
                "Opened project '{}' from CLI: '{}' ({} assets, git: {}, engine: {})",
                project_name.as_deref().unwrap_or("<unknown>"),
                path.display(),
                state.asset_entries.len(),
                git_branch.as_deref().unwrap_or("none"),
                project_engine_version.as_deref().unwrap_or("<unknown>")
            );
        }

        // Auto-load (or create) the default scene through the VFS so the
        // new file shows up in the next index.
        scene_io::auto_load_or_create_default_scene(&mut pvfs, world);

        // Refresh the asset browser cache after the potential
        // default-scene seed.
        let entries: Vec<AssetEntry> = hot_reload::collect_asset_entries(&pvfs);
        let dirs = pvfs.list_dirs();
        if let Ok(mut state) = self.editor_state.lock() {
            state.asset_entries = entries;
            state.asset_dirs = dirs;
            state.asset_epoch = state.asset_epoch.wrapping_add(1);
        }

        self.project_vfs = Some(Arc::new(Mutex::new(pvfs)));
    }
}
