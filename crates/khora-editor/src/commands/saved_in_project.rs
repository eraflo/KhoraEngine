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

//! Where a Save As of a scene or a prefab writes: a file of its kind under the open project's assets, or nowhere.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use khora_sdk::khora_core::asset::AssetKind;

use crate::project_vfs::{InProject, ProjectVfs};

/// Names the file at `picked` as the file of the open project a Save As of a `kind` writes; `None`, logged with why, when it is none.
///
/// Returns the project's assets folder and the file's forward-slash key under
/// it. A name picked with no extension takes the kind's
/// ([`extension_for_kind`](khora_sdk::extension_for_kind)), as a file dialog
/// adds it: `levels/forest` saves a scene as `levels/forest.kscene`. Refused:
/// no project open ([`no_project`]), a file outside the project's `assets/` (a
/// scene or a prefab is saved in its project, never elsewhere), a path under
/// the assets the project refuses ([`InProject::Refused`]), or a file its
/// extension, in any case, does not make a `kind`: another kind's, such as a
/// texture's `wall.png`, which is never written over, or one of no kind.
pub(crate) fn saved_in_project(
    project_vfs: Option<&Arc<Mutex<ProjectVfs>>>,
    picked: &Path,
    kind: AssetKind,
) -> Option<(PathBuf, String)> {
    let what = kind.name();
    let Some(pvfs) = project_vfs.and_then(|pvfs| pvfs.lock().ok()) else {
        log::error!("The {what} is not saved: {}", no_project(what));
        return None;
    };
    let picked = with_extension_of(picked, kind);
    let key = match pvfs.in_project(&picked) {
        InProject::Inside(key) => key,
        InProject::Refused(why) => {
            log::error!("The {what} is not saved: {why}");
            return None;
        }
        InProject::Outside => {
            log::error!(
                "The {what} is not saved: '{}' is outside the project's assets '{}', where its \
                 {what}s are saved",
                picked.display(),
                pvfs.assets_root.display()
            );
            return None;
        }
    };
    if let Err(why) = of_kind(&key, kind) {
        log::error!("The {what} is not saved: {why}");
        return None;
    }
    Some((pvfs.assets_root.clone(), key))
}

/// Returns why a `what` (a scene, a prefab) is not saved with no project open.
pub(crate) fn no_project(what: &str) -> String {
    format!("no project is open, and a {what} is saved under a project's assets")
}

/// Returns the open project's assets folder, where a Save As dialog starts; `None` with no project open.
pub(crate) fn assets_folder(project_vfs: Option<&Arc<Mutex<ProjectVfs>>>) -> Option<PathBuf> {
    project_vfs.and_then(|pvfs| pvfs.lock().ok().map(|pvfs| pvfs.assets_root.clone()))
}

/// Returns `picked`, given the extension of `kind` when its name has none.
fn with_extension_of(picked: &Path, kind: AssetKind) -> PathBuf {
    if picked.extension().is_some() || picked.file_name().is_none() {
        return picked.to_path_buf();
    }
    let mut named = picked.as_os_str().to_owned();
    named.push(".");
    named.push(khora_sdk::extension_for_kind(kind));
    PathBuf::from(named)
}

/// Checks that the file at the forward-slash `key` is a `kind` by its extension; `Err` says what it is instead.
fn of_kind(key: &str, kind: AssetKind) -> Result<(), String> {
    let what = kind.name();
    let extension = khora_sdk::extension_for_kind(kind);
    match khora_sdk::kind_of_path(Path::new(key)) {
        Some(found) if found == kind => Ok(()),
        Some(other) => Err(format!(
            "'{key}' is a {other} file, never written over by a {what}: a {what} file ends in \
             .{extension}"
        )),
        None => Err(format!(
            "'{key}' is no {what} file: a {what} file ends in .{extension}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use khora_sdk::khora_core::asset::kinds;
    use khora_sdk::MetricsRegistry;

    use super::{assets_folder, no_project, saved_in_project};
    use crate::project_vfs::ProjectVfs;

    /// A project on disk, removed when the test ends.
    struct Project(std::path::PathBuf, Arc<Mutex<ProjectVfs>>);

    impl Drop for Project {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn project(name: &str) -> Project {
        let root = std::env::temp_dir().join(format!(
            "khora-editor-saved-in-project-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("assets")).expect("the assets folder");
        let vfs = ProjectVfs::open(root.clone(), Arc::new(MetricsRegistry::new()))
            .expect("the project opens");
        Project(root, Arc::new(Mutex::new(vfs)))
    }

    /// A file under the assets is named by its key, under the project's assets folder.
    #[test]
    fn saved_in_project_names_a_file_under_the_assets_by_its_key() {
        let project = project("inside");
        let assets = project.0.join("assets");

        let named = saved_in_project(
            Some(&project.1),
            &assets.join("prefabs").join("crate.kprefab"),
            kinds::PREFAB,
        );

        assert_eq!(named, Some((assets, "prefabs/crate.kprefab".to_owned())));
    }

    /// A name picked with no extension takes the kind's, as a file dialog adds it.
    #[test]
    fn saved_in_project_gives_a_name_with_no_extension_the_kinds() {
        let project = project("no-extension");
        let assets = project.0.join("assets");

        let named = saved_in_project(
            Some(&project.1),
            &assets.join("levels").join("forest"),
            kinds::SCENE,
        );

        assert_eq!(named, Some((assets, "levels/forest.kscene".to_owned())));
    }

    /// The kind is read from the extension in any case.
    #[test]
    fn saved_in_project_reads_the_kind_in_any_case() {
        let project = project("any-case");
        let assets = project.0.join("assets");

        let named = saved_in_project(Some(&project.1), &assets.join("Level.KSCENE"), kinds::SCENE);

        assert_eq!(named, Some((assets, "Level.KSCENE".to_owned())));
    }

    /// A file of another kind, or of none, is no file a save of a kind writes.
    #[test]
    fn saved_in_project_refuses_a_file_of_another_kind_or_none() {
        let project = project("other-kind");
        let assets = project.0.join("assets");

        for picked in ["wall.png", "crate.kprefab", "notes.txt", "level.scn"] {
            assert_eq!(
                saved_in_project(Some(&project.1), &assets.join(picked), kinds::SCENE),
                None,
                "{picked}"
            );
        }
    }

    /// A file outside the assets, or any with no project open, is none.
    #[test]
    fn saved_in_project_refuses_outside_the_assets_and_with_no_project() {
        let project = project("outside");

        assert_eq!(
            saved_in_project(
                Some(&project.1),
                &project.0.join("a.kprefab"),
                kinds::PREFAB
            ),
            None
        );
        assert_eq!(
            saved_in_project(
                None,
                &project.0.join("assets").join("a.kprefab"),
                kinds::PREFAB
            ),
            None
        );
    }

    /// Why nothing is saved with no project open names what is not saved.
    #[test]
    fn no_project_names_what_is_not_saved() {
        assert!(no_project("prefab").contains("a prefab is saved"));
    }

    /// The dialog starts in the project's assets folder; with no project, nowhere.
    #[test]
    fn assets_folder_is_the_open_projects_assets() {
        let project = project("assets-folder");

        assert_eq!(
            assets_folder(Some(&project.1)).as_deref(),
            Some(project.0.join("assets").as_path())
        );
        assert_eq!(assets_folder(None), None);
    }
}
