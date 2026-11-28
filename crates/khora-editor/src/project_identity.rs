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

//! A project's game identity, read from its `project.json`: the name the
//! editor runs the game by, and the version "Build Game" stamps on it.

use khora_sdk::trust::{FileGuard, ProjectFile};
use khora_sdk::GameIdentity;

/// Returns the game the project `project` guards is, from its `project.json`'s `name` and `version`.
///
/// A file without a `version` reads `0.1.0`. `None` when the project has no
/// `project.json`, or one the guard refuses (reported to its refusals), that
/// does not parse or names no game.
pub fn project_identity(project: &FileGuard) -> Option<GameIdentity> {
    let json = read_project_json(project)?;
    let name = field(&json, "name")?;
    Some(GameIdentity {
        name,
        version: version_of(&json),
    })
}

/// Returns the version of the game `project` guards: its `project.json`'s `version`,
/// else `0.1.0`, whether or not the file names the game.
pub fn project_version(project: &FileGuard) -> String {
    read_project_json(project).map_or_else(
        || khora_sdk::DEFAULT_GAME_VERSION.to_owned(),
        |json| version_of(&json),
    )
}

/// Reads and parses the `project.json` of the project whose root `project` guards.
///
/// `None` when it has none; when the guard refuses it, the refusal reported to
/// the guard's refusals; when it cannot be read or does not parse, each logged.
pub fn read_project_json(project: &FileGuard) -> Option<serde_json::Value> {
    let bytes = match project.read_project_file(ProjectFile::Manifest) {
        Ok(bytes) => bytes,
        Err(error) if error.is_missing() => return None,
        Err(error) => {
            log::warn!(
                "{} is not read: {error}",
                project
                    .root()
                    .join(ProjectFile::Manifest.rel_path())
                    .display()
            );
            return None;
        }
    };
    match serde_json::from_slice(&bytes) {
        Ok(json) => Some(json),
        Err(error) => {
            log::warn!(
                "{} does not parse: {error}",
                project
                    .root()
                    .join(ProjectFile::Manifest.rel_path())
                    .display()
            );
            None
        }
    }
}

/// Returns `json`'s `key`, trimmed, when it is a string not blank.
pub fn field(json: &serde_json::Value, key: &str) -> Option<String> {
    (json.get(key).and_then(|value| value.as_str()))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// Returns `json`'s `version`, trimmed; `0.1.0` when it names none.
fn version_of(json: &serde_json::Value) -> String {
    field(json, "version").unwrap_or_else(|| khora_sdk::DEFAULT_GAME_VERSION.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Arc;

    use khora_sdk::trust::{Refusals, SecurityReason, Trust};

    /// A guard over `root` at a trusted project's trust, its refusals kept in memory.
    fn trusted(root: &std::path::Path) -> FileGuard {
        FileGuard::new(root, Trust::Project, Refusals::detached())
    }

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
            project_identity(&trusted(&project.0)),
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
            project_identity(&trusted(&project.0)),
            Some(GameIdentity {
                name: "Lantern".to_owned(),
                version: "2.3.4".to_owned(),
            })
        );
    }

    /// **No game is named without a project file.**
    ///
    /// A folder with no `project.json`, or one that does not parse, names no
    /// game: the editor falls back to its own default.
    #[test]
    fn no_game_is_named_without_a_readable_project_file() {
        let missing = Project::with("missing", None);
        let broken = Project::with("broken", Some("{ not json"));

        assert_eq!(
            project_identity(&trusted(&missing.0)),
            None,
            "no `project.json`"
        );
        assert_eq!(
            project_identity(&trusted(&broken.0)),
            None,
            "a `project.json` that does not parse"
        );
    }

    /// **A `project.json` over its cap names no game, and its refusal is reported.**
    ///
    /// Read before the user decided whether to trust the project, it is read
    /// under the strict cap.
    #[test]
    fn a_project_file_over_its_cap_names_no_game_and_is_reported() {
        let project = Project::with("oversized", None);
        let path = project.0.join("project.json");
        std::fs::write(&path, r#"{ "name": "Lantern" }"#).expect("writes");
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .and_then(|file| file.set_len((256 << 10) + 1))
            .expect("the file grows past the strict cap");
        let refusals = Arc::new(Refusals::new(None, None));
        let strict = FileGuard::new(&project.0, Trust::Restricted, Arc::clone(&refusals));

        assert_eq!(project_identity(&strict), None);
        assert!(
            refusals.quarantined().iter().any(|refusal| {
                refusal.path.ends_with("project.json")
                    && matches!(refusal.reason, SecurityReason::TooLarge { .. })
            }),
            "{:?}",
            refusals.quarantined()
        );
    }

    /// **`project.json` is read and parsed whole**; one missing or that does
    /// not parse reads as none.
    #[test]
    fn read_project_json_parses_the_file_or_reads_none() {
        let named = Project::with("parsed", Some(r#"{ "name": "Lantern" }"#));
        let missing = Project::with("parsed-missing", None);
        let broken = Project::with("parsed-broken", Some("{ not json"));

        let json = read_project_json(&trusted(&named.0)).expect("the file parses");
        assert_eq!(field(&json, "name").as_deref(), Some("Lantern"));
        assert!(read_project_json(&trusted(&missing.0)).is_none());
        assert!(read_project_json(&trusted(&broken.0)).is_none());
    }
}
