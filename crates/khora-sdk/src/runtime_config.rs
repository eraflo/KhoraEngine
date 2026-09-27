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

//! The `runtime.json` schema: what the editor's "Build Game" writes next to
//! the runtime binary, and what [`crate::run_default`] reads at start-up.
//! One type for both sides, so the writer and the reader cannot drift.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Name of the config file, next to the runtime binary.
pub const RUNTIME_CONFIG_FILE: &str = "runtime.json";

/// Project-relative path of the scene loaded when `runtime.json` names none,
/// and the scene the editor creates by default.
pub const DEFAULT_SCENE_REL_PATH: &str = "scenes/default.kscene";

/// Runtime config the launcher (editor's "Build Game") drops next to the
/// binary. Read at startup; sensible defaults are used when the file is
/// missing (typical for engine contributors running the runtime against a
/// loose `assets/` directory).
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
    /// Build preset label written by the editor (debug/release/shipping).
    /// Optional for older runtime.json files. Used for diagnostics.
    #[serde(default)]
    pub preset: Option<String>,
    /// When `true`, the runtime hashes each loaded asset against
    /// `manifest.bin` and aborts on mismatch. Defaults to `false` so
    /// older packs (without a manifest) keep booting.
    #[serde(default)]
    pub verify_integrity: bool,
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

    /// The config of a runtime started without `runtime.json`.
    pub fn defaults() -> Self {
        Self {
            project_name: default_project_name(),
            default_scene: default_scene_rel_path(),
            window_title: None,
            preset: None,
            verify_integrity: false,
        }
    }

    /// The window title: the `window_title` field if set, else the project name.
    pub fn window_title(&self) -> String {
        self.window_title
            .clone()
            .unwrap_or_else(|| self.project_name.clone())
    }
}
