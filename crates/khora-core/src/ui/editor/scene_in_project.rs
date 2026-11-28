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

//! A scene file named inside the project it belongs to.

use std::path::{Path, PathBuf};

/// A scene file of a project: the project's assets folder and the file's forward-slash key under it.
///
/// What the editor saves its open scene to. A save goes through the project
/// whose assets folder it names, by its key: a scene of another project is
/// refused, never written. It reads as the file's path
/// (`<assets>/<key>`), for what shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneInProject {
    /// The assets folder of the project the scene belongs to.
    assets: PathBuf,
    /// The file's forward-slash path under `assets`.
    key: String,
    /// The file's path: `key` under `assets`.
    path: PathBuf,
}

impl SceneInProject {
    /// Names the scene file at the forward-slash `key` under the project assets folder `assets`.
    pub fn new(assets: impl Into<PathBuf>, key: impl Into<String>) -> Self {
        let assets = assets.into();
        let key = key.into();
        let path = key
            .split('/')
            .fold(assets.clone(), |path, part| path.join(part));
        Self { assets, key, path }
    }

    /// Returns the assets folder of the project the scene belongs to.
    pub fn assets(&self) -> &Path {
        &self.assets
    }

    /// Returns the scene file's forward-slash path under its project's assets folder.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Returns the scene file's path: its key under its project's assets folder.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns whether the scene belongs to the project whose assets folder is `assets`.
    pub fn is_in(&self, assets: &Path) -> bool {
        self.assets == assets
    }
}

impl std::ops::Deref for SceneInProject {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<Path> for SceneInProject {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::SceneInProject;

    /// The scene's path is its key under its project's assets folder, part by part.
    #[test]
    fn the_path_is_the_key_under_the_assets_folder() {
        let assets = Path::new("project").join("assets");
        let scene = SceneInProject::new(&assets, "scenes/level.kscene");

        assert_eq!(scene.path(), assets.join("scenes").join("level.kscene"));
        assert_eq!(scene.key(), "scenes/level.kscene");
        assert_eq!(scene.assets(), assets);
        assert!(
            scene.starts_with(Path::new("project")),
            "it reads as its path"
        );
    }

    /// A scene belongs to the project whose assets folder it names, and to no other.
    #[test]
    fn a_scene_is_in_its_project_only() {
        let scene = SceneInProject::new(Path::new("a").join("assets"), "scenes/level.kscene");

        assert!(scene.is_in(&Path::new("a").join("assets")));
        assert!(!scene.is_in(&Path::new("b").join("assets")));
    }
}
