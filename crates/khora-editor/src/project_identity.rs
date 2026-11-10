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

//! A project's game identity, read from its `project.json`: what the editor
//! names the game it runs by, and what "Build Game" versions the built game
//! with.

use std::path::Path;

use khora_sdk::GameIdentity;

/// The game the project at `root` is: the `name` and `version` of its
/// `project.json`, the version `0.1.0` for a project written before projects
/// had one. `None` when the project has no `project.json`, or one that does
/// not parse or names no game.
pub fn project_identity(root: &Path) -> Option<GameIdentity> {
    let json = project_json(root)?;
    let name = field(&json, "name")?;
    Some(GameIdentity {
        name,
        version: version_of(&json),
    })
}

/// The version of the game the project at `root` is: its `project.json`'s
/// `version`, else `0.1.0` — whether or not the file names the game.
pub fn project_version(root: &Path) -> String {
    project_json(root).map_or_else(
        || khora_sdk::DEFAULT_GAME_VERSION.to_owned(),
        |json| version_of(&json),
    )
}

/// The project at `root`'s `project.json`, parsed; `None` when it has none
/// or one that does not parse.
fn project_json(root: &Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(root.join("project.json")).ok()?;
    serde_json::from_str(&text).ok()
}

/// `json`'s `key`, trimmed, when it is a string not blank.
pub fn field(json: &serde_json::Value, key: &str) -> Option<String> {
    (json.get(key).and_then(|value| value.as_str()))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// `json`'s `version`, trimmed; `0.1.0` when it names none.
fn version_of(json: &serde_json::Value) -> String {
    field(json, "version").unwrap_or_else(|| khora_sdk::DEFAULT_GAME_VERSION.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A project folder under the temp folder holding `project_json` (none
    /// when `None`), removed when dropped.
    struct Project(PathBuf);

    impl Project {
        fn with(tag: &str, project_json: Option<&str>) -> Self {
            let root = std::env::temp_dir().join(format!(
                "khora-editor-project-identity-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).expect("the project folder");
            if let Some(text) = project_json {
                std::fs::write(root.join("project.json"), text).expect("the `project.json`");
            }
            Project(root)
        }
    }

    impl Drop for Project {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// **An older project reads version 0.1.0.** A `project.json` written
    /// before projects had a version names its game at `0.1.0`.
    #[test]
    fn an_older_project_reads_version_0_1_0() {
        let project = Project::with(
            "older",
            Some(r#"{ "name": "Lantern", "engine_version": "0.9.1", "created_at": 0 }"#),
        );

        assert_eq!(
            project_identity(&project.0),
            Some(GameIdentity {
                name: "Lantern".to_owned(),
                version: "0.1.0".to_owned(),
            })
        );
    }

    /// **A project names its game's version.** The `version` of its
    /// `project.json` is the game's, as written.
    #[test]
    fn a_project_names_its_games_version() {
        let project = Project::with(
            "versioned",
            Some(r#"{ "name": "Lantern", "version": "2.3.4", "engine_version": "0.9.1" }"#),
        );

        assert_eq!(
            project_identity(&project.0),
            Some(GameIdentity {
                name: "Lantern".to_owned(),
                version: "2.3.4".to_owned(),
            })
        );
    }

    /// **No game is named without a project file.** A folder with no
    /// `project.json`, or one that does not parse, names no game: the editor
    /// falls back to its own default.
    #[test]
    fn no_game_is_named_without_a_readable_project_file() {
        let missing = Project::with("missing", None);
        let broken = Project::with("broken", Some("{ not json"));

        assert_eq!(project_identity(&missing.0), None, "no `project.json`");
        assert_eq!(
            project_identity(&broken.0),
            None,
            "a `project.json` that does not parse"
        );
    }
}
