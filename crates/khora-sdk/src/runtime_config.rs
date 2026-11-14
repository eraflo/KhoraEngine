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

//! The `runtime.json` schema, written by the editor's Build Game and read by [`crate::run_default()`].
//!
//! One type serves both sides, so the writer and the reader cannot drift.

use khora_core::physics::PhysicsSettings;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Name of the config file, next to the runtime binary.
pub const RUNTIME_CONFIG_FILE: &str = "runtime.json";

/// Project-relative path of the scene loaded when `runtime.json` names none,
/// and the scene the editor creates by default.
pub const DEFAULT_SCENE_REL_PATH: &str = "scenes/default.kscene";

/// The `runtime.json` the editor's Build Game writes next to the runtime binary.
///
/// A missing file means the defaults, as when the runtime runs against a
/// loose `assets/` folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeConfig {
    /// Project name, shown in logs and used as the window title by default.
    #[serde(default = "default_project_name")]
    pub project_name: String,
    /// Project-relative path of the scene loaded at start-up.
    #[serde(default = "default_scene_rel_path")]
    pub default_scene: String,
    /// Window title; the project name when absent.
    #[serde(default)]
    pub window_title: Option<String>,
    /// Build preset label written by the editor (debug/release/shipping), for diagnostics.
    #[serde(default)]
    pub preset: Option<String>,
    /// The game's physics settings (`physics.step`). Written only when the
    /// project sets them; a file without them runs at the defaults.
    #[serde(default, skip_serializing_if = "PhysicsSettings::is_default")]
    pub physics: PhysicsSettings,
    /// The game's version, from the project's `project.json`; [`DEFAULT_GAME_VERSION`] when absent.
    #[serde(default = "default_game_version")]
    pub game_version: String,
}

/// The version of a game that names none.
pub const DEFAULT_GAME_VERSION: &str = "0.1.0";

fn default_game_version() -> String {
    DEFAULT_GAME_VERSION.to_owned()
}

fn default_project_name() -> String {
    "Khora Runtime".to_owned()
}

fn default_scene_rel_path() -> String {
    DEFAULT_SCENE_REL_PATH.to_owned()
}

impl RuntimeConfig {
    /// Reads `<exe_dir>/runtime.json`, or the defaults when it is missing or
    /// malformed.
    pub fn load_or_default(exe_dir: &Path) -> Self {
        let path = exe_dir.join(RUNTIME_CONFIG_FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<RuntimeConfig>(&text) {
                Ok(cfg) => {
                    log::info!(
                        "khora-sdk run_default: loaded {} (project='{}', scene='{}', preset={})",
                        path.display(),
                        cfg.project_name,
                        cfg.default_scene,
                        cfg.preset.as_deref().unwrap_or("<unspecified>")
                    );
                    cfg
                }
                Err(e) => {
                    log::warn!(
                        "khora-sdk run_default: malformed runtime.json ({}): {} — \
                         falling back to defaults",
                        path.display(),
                        e
                    );
                    Self::defaults()
                }
            },
            Err(_) => {
                log::info!(
                    "khora-sdk run_default: no runtime.json at {} — running in dev \
                     mode with defaults",
                    path.display()
                );
                Self::defaults()
            }
        }
    }

    /// Returns the config of a runtime started without `runtime.json`.
    pub fn defaults() -> Self {
        Self {
            project_name: default_project_name(),
            default_scene: default_scene_rel_path(),
            window_title: None,
            preset: None,
            physics: PhysicsSettings::default(),
            game_version: default_game_version(),
        }
    }

    /// Returns the game this config runs: its `project_name` at its `game_version`.
    pub fn game_identity(&self) -> khora_core::app::GameIdentity {
        let version = self.game_version.trim();
        khora_core::app::GameIdentity {
            name: self.project_name.clone(),
            // A blank version names none: the default one.
            version: if version.is_empty() {
                DEFAULT_GAME_VERSION.to_owned()
            } else {
                version.to_owned()
            },
        }
    }

    /// Returns the window title: `window_title` if set, else the project name.
    pub fn window_title(&self) -> String {
        self.window_title
            .clone()
            .unwrap_or_else(|| self.project_name.clone())
    }
}
