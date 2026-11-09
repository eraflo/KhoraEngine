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

//! A project's physics settings, read from its `project.json` before the
//! engine boots: the engine boots its physics family at their step.

use std::path::Path;

use khora_sdk::khora_core::physics::PhysicsSettings;

/// The physics settings of the project at `root`: the `physics` section of
/// its `project.json`, or the defaults when it has none or the file cannot be
/// read. A section that does not read (a zero or negative step) is reported
/// and the defaults kept. A few hundred bytes, read as the editor starts:
/// the engine boots its physics family at this step.
pub fn project_physics(root: &Path) -> PhysicsSettings {
    let path = root.join("project.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return PhysicsSettings::default();
    };
    let Ok(project) = serde_json::from_str::<serde_json::Value>(&text) else {
        log::warn!(
            "{} does not parse: its physics settings are ignored",
            path.display()
        );
        return PhysicsSettings::default();
    };
    match project.get("physics") {
        None => PhysicsSettings::default(),
        Some(section) => serde_json::from_value(section.clone()).unwrap_or_else(|error| {
            log::warn!("project.json: the `physics` section is ignored: {error}");
            PhysicsSettings::default()
        }),
    }
}
