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

//! Which `.erg` files are scripts, and the module each is known by.
//!
//! An `.erg` file is a script when its path under the assets root starts with
//! the folder [`SCRIPT_DIR`], spelled exactly — the same answer on every
//! operating system, whatever its file system thinks of case. Its module is
//! its path after that folder, with forward slashes, and its extension is
//! `.erg` in lower case — the spelling an `import` must use. Any other Ergon
//! file, outside that folder or spelled `.ERG`, is *misplaced*: never
//! compiled, and reported.

use std::path::{Path, PathBuf};

/// The folder of an assets root that holds its scripts, matched exactly.
pub const SCRIPT_DIR: &str = "scripts";

/// Whether `path` names an Ergon file — the extension `.erg` matched without
/// regard to case, so a `.ERG` is recognised and can be reported rather than
/// ignored. Either separator.
pub fn is_script_file(path: &str) -> bool {
    extension_of(path)
        .is_some_and(|extension| extension.eq_ignore_ascii_case(khora_script::modules::EXTENSION))
}

/// Whether `path` names a file a module can be: its extension exactly `.erg`,
/// in lower case — the one spelling the compiler accepts in an import. Where
/// it is decides the rest ([`module_of`]). Either separator.
pub fn is_module_file(path: &str) -> bool {
    extension_of(path) == Some(khora_script::modules::EXTENSION)
}

/// The extension of the file `path` names, when it has a stem before it.
fn extension_of(path: &str) -> Option<&str> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    name.rsplit_once('.')
        .filter(|(stem, _)| !stem.is_empty())
        .map(|(_, extension)| extension)
}

/// The module the file at `rel_path` (under the assets root, forward slashes)
/// is known by: `scripts/ai/guard.erg` is `ai/guard.erg`. `None` for a file
/// whose extension is not exactly `.erg` — the compiler refuses any other
/// spelling in an import — for anything not under the exact [`SCRIPT_DIR`],
/// and for that folder with nothing after it.
pub fn module_of(rel_path: &str) -> Option<&str> {
    if !is_module_file(rel_path) {
        return None;
    }
    rel_path
        .strip_prefix(SCRIPT_DIR)?
        .strip_prefix('/')
        .filter(|module| !module.is_empty())
}

/// `assets_root`'s script folder, when an entry named exactly [`SCRIPT_DIR`]
/// is a directory there.
///
/// Read from the directory's listing rather than by joining the name: on a
/// file system that ignores case, `Scripts` would answer to `scripts` — and a
/// project mounted on one system would lose its scripts on another.
pub fn script_dir(assets_root: &Path) -> Option<PathBuf> {
    std::fs::read_dir(assets_root)
        .ok()?
        .filter_map(Result::ok)
        .find(|entry| entry.file_name() == SCRIPT_DIR)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
}

/// Whether `rel_path` (forward slashes) names an entry under `base` spelled
/// exactly so, folder by folder — on a file system that ignores case too.
///
/// What a module is depends on spelling (`scripts/`, `Guard.erg`), so whether
/// its file is still there must too: on such a file system, `Scripts/` or
/// `guard.erg` would answer for a file renamed away from the spelling the
/// module goes by, and the module would never be known to be gone.
pub fn exists_exactly(base: &Path, rel_path: &str) -> bool {
    let mut at = base.to_path_buf();
    for part in rel_path.split('/').filter(|part| !part.is_empty()) {
        let listed = std::fs::read_dir(&at)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .any(|entry| entry.file_name() == part)
            })
            .unwrap_or(false);
        if !listed {
            return false;
        }
        at.push(part);
    }
    true
}

/// Whether `rel_path` under `base` is a file spelled exactly so — see
/// [`exists_exactly`].
pub fn is_file_exactly(base: &Path, rel_path: &str) -> bool {
    exists_exactly(base, rel_path) && base.join(rel_path).is_file()
}

/// Every Ergon file under `assets_root` that is no module — outside its script
/// folder, or spelled `.ERG` in it — by its path under the root — forward slashes, sorted. Empty for a root that
/// does not exist. Links are not followed.
pub fn misplaced(assets_root: &Path) -> Vec<String> {
    let mut found: Vec<String> = walkdir::WalkDir::new(assets_root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        // A file the asset scan and the watch skip — hidden, a swap file — is
        // one the engine never hears of again: listing it would leave an error
        // that no deletion could take back.
        .filter(|entry| {
            entry.file_type().is_file()
                && !crate::asset::should_skip_file(&entry.file_name().to_string_lossy())
        })
        .filter_map(|entry| {
            let relative = entry.path().strip_prefix(assets_root).ok()?;
            Some(relative.to_string_lossy().replace('\\', "/"))
        })
        .filter(|rel_path| is_script_file(rel_path) && module_of(rel_path).is_none())
        .collect();
    found.sort();
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes an empty file at `rel_path` under `root`, making its folders.
    fn touch(root: &Path, rel_path: &str) {
        let path = root.join(rel_path);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("makes the folders");
        std::fs::write(&path, "fn void Main() { }").expect("writes");
    }

    // ─── module_of ──────────────────────────────────────────────────────────

    /// **A module drops the script folder**: an import names it from there.
    #[test]
    fn a_nested_script_is_known_by_its_path_under_the_script_folder() {
        assert_eq!(module_of("scripts/ai/guard.erg"), Some("ai/guard.erg"));
        assert_eq!(
            module_of("scripts/combat/melee/sword.erg"),
            Some("combat/melee/sword.erg")
        );
    }

    #[test]
    fn a_script_directly_in_the_script_folder_is_known_by_its_file_name() {
        assert_eq!(module_of("scripts/guard.erg"), Some("guard.erg"));
    }

    /// **A file at the top of the assets is no module**: the assets root is
    /// not a script root.
    #[test]
    fn an_erg_at_the_top_level_of_the_assets_is_no_module() {
        assert_eq!(module_of("guard.erg"), None);
    }

    /// **Another folder is not a script root**: `props/lantern.erg` is a
    /// misplaced file, not the module `lantern.erg`.
    #[test]
    fn an_erg_under_another_folder_is_no_module() {
        assert_eq!(module_of("props/lantern.erg"), None);
        assert_eq!(module_of("textures/wood/notes.erg"), None);
    }

    /// **The folder's name is matched exactly**, so a project gives the same
    /// answer on a file system that ignores case as on one that does not.
    #[test]
    fn a_script_folder_spelled_in_another_case_is_no_script_root() {
        assert_eq!(module_of("Scripts/guard.erg"), None);
        assert_eq!(module_of("SCRIPTS/ai/guard.erg"), None);
    }

    /// A folder whose name merely starts with `scripts` is another folder.
    #[test]
    fn a_folder_whose_name_starts_with_scripts_is_no_script_root() {
        assert_eq!(module_of("scriptsextra/guard.erg"), None);
        assert_eq!(module_of("scripts.old/guard.erg"), None);
    }

    /// Only the first folder counts: a `scripts` folder deeper down is inside
    /// some other folder.
    #[test]
    fn a_scripts_folder_below_another_folder_is_no_script_root() {
        assert_eq!(module_of("props/scripts/x.erg"), None);
    }

    #[test]
    fn the_script_folder_alone_is_no_module() {
        assert_eq!(module_of("scripts/"), None);
        assert_eq!(module_of("scripts"), None);
    }

    /// **A module's extension is `.erg` in lower case**, the one spelling the
    /// compiler accepts in an import: `Guard.ERG` is an Ergon file but no
    /// module — reported as misplaced rather than compiled to an error.
    #[test]
    fn an_extension_in_capitals_is_an_ergon_file_but_no_module() {
        assert_eq!(module_of("scripts/ai/Guard.ERG"), None);
        assert_eq!(module_of("scripts/lamp.Erg"), None);
        assert!(is_script_file("scripts/ai/Guard.ERG"));
    }

    #[test]
    fn an_extension_in_capitals_in_the_script_folder_is_misplaced() {
        let root = tempfile::TempDir::new().expect("a temp dir");
        touch(root.path(), "scripts/ai/Guard.ERG");
        touch(root.path(), "scripts/ai/guard.erg");

        assert_eq!(
            misplaced(root.path()),
            vec!["scripts/ai/Guard.ERG".to_owned()]
        );
    }

    #[test]
    fn a_file_that_is_not_ergon_is_no_module() {
        assert_eq!(module_of("scripts/notes.txt"), None);
        assert_eq!(module_of("scripts/ai"), None);
        assert_eq!(module_of("scripts/guard.erg.bak"), None);
        assert_eq!(module_of("scripts/erg"), None);
        assert_eq!(module_of("textures/wood.png"), None);
    }

    // ─── is_script_file ─────────────────────────────────────────────────────

    #[test]
    fn an_erg_file_is_a_script_file_in_any_case_and_with_either_separator() {
        assert!(is_script_file("guard.erg"));
        assert!(is_script_file("scripts/ai/guard.erg"));
        assert!(is_script_file("props/Lantern.ERG"));
        assert!(is_script_file("scripts\\ai\\guard.Erg"));
    }

    #[test]
    fn a_file_with_another_extension_or_none_is_no_script_file() {
        assert!(!is_script_file("textures/wood.png"));
        assert!(!is_script_file("scripts/guard.erg.bak"));
        assert!(!is_script_file("scripts/erg"));
        assert!(!is_script_file("scripts/ai"));
        assert!(!is_script_file("scripts"));
        assert!(!is_script_file(""));
    }

    // ─── script_dir ─────────────────────────────────────────────────────────

    #[test]
    fn the_script_folder_is_found_when_it_exists() {
        let root = tempfile::TempDir::new().expect("a temp dir");
        std::fs::create_dir_all(root.path().join("scripts")).expect("makes the folder");

        assert_eq!(script_dir(root.path()), Some(root.path().join("scripts")));
    }

    /// **Spelled exactly, on every file system**: a `Scripts` folder is found
    /// by name on Windows if asked for `scripts` — and must not be.
    #[test]
    fn a_script_folder_spelled_in_another_case_is_not_found() {
        let root = tempfile::TempDir::new().expect("a temp dir");
        std::fs::create_dir_all(root.path().join("Scripts")).expect("makes the folder");

        assert_eq!(script_dir(root.path()), None);
    }

    #[test]
    fn a_root_without_a_script_folder_has_none() {
        let root = tempfile::TempDir::new().expect("a temp dir");
        std::fs::create_dir_all(root.path().join("props")).expect("makes a folder");

        assert_eq!(script_dir(root.path()), None);
    }

    /// A file named `scripts` holds no scripts.
    #[test]
    fn a_file_named_scripts_is_no_script_folder() {
        let root = tempfile::TempDir::new().expect("a temp dir");
        std::fs::write(root.path().join("scripts"), "not a folder").expect("writes");

        assert_eq!(script_dir(root.path()), None);
    }

    #[test]
    fn a_missing_root_has_no_script_folder() {
        let root = tempfile::TempDir::new().expect("a temp dir");

        assert_eq!(script_dir(&root.path().join("gone")), None);
    }

    // ─── misplaced ──────────────────────────────────────────────────────────

    /// **Every `.erg` outside the script folder, and only those**: by path
    /// under the root, forward slashes, sorted — whatever the case of their
    /// extension, however deep, a `scripts` folder below another included.
    #[test]
    fn misplaced_lists_every_erg_outside_the_script_folder_sorted() {
        let root = tempfile::TempDir::new().expect("a temp dir");
        touch(root.path(), "scripts/main.erg");
        touch(root.path(), "scripts/ai/guard.erg");
        touch(root.path(), "props/lantern.erg");
        touch(root.path(), "guard.erg");
        touch(root.path(), "deep/a/b/c.ERG");
        touch(root.path(), "props/scripts/x.erg");
        touch(root.path(), "textures/wood.png");

        assert_eq!(
            misplaced(root.path()),
            vec![
                "deep/a/b/c.ERG".to_owned(),
                "guard.erg".to_owned(),
                "props/lantern.erg".to_owned(),
                "props/scripts/x.erg".to_owned(),
            ]
        );
    }

    /// A script folder spelled in another case is not the script folder: what
    /// it holds is misplaced.
    #[test]
    fn misplaced_lists_what_a_script_folder_in_another_case_holds() {
        let root = tempfile::TempDir::new().expect("a temp dir");
        touch(root.path(), "Scripts/ai/guard.erg");

        assert_eq!(
            misplaced(root.path()),
            vec!["Scripts/ai/guard.erg".to_owned()]
        );
    }

    #[test]
    fn a_root_with_every_script_in_place_has_nothing_misplaced() {
        let root = tempfile::TempDir::new().expect("a temp dir");
        touch(root.path(), "scripts/main.erg");
        touch(root.path(), "scripts/ai/guard.erg");
        touch(root.path(), "textures/wood.png");

        assert!(misplaced(root.path()).is_empty());
    }

    /// A folder whose name ends in `.erg` is a folder, not a file.
    #[test]
    fn a_folder_named_like_a_script_is_not_misplaced() {
        let root = tempfile::TempDir::new().expect("a temp dir");
        std::fs::create_dir_all(root.path().join("props/odd.erg")).expect("makes the folder");

        assert!(misplaced(root.path()).is_empty());
    }

    #[test]
    fn a_missing_root_has_nothing_misplaced() {
        let root = tempfile::TempDir::new().expect("a temp dir");

        assert!(misplaced(&root.path().join("gone")).is_empty());
    }

    /// **A module file is `.erg` exactly**: the one spelling an import takes.
    #[test]
    fn only_a_lower_case_erg_is_a_module_file() {
        assert!(is_module_file("guard.erg"));
        assert!(is_module_file(r"scripts\ai\guard.erg"));
        assert!(!is_module_file("scripts/ai/Guard.ERG"));
        assert!(!is_module_file("scripts/lamp.Erg"));
        assert!(!is_module_file("scripts/.erg"));
        assert!(!is_module_file("textures/wood.png"));
    }

    /// **Existence is checked by spelling**, so a file renamed to another
    /// case is gone under its old name on every file system.
    #[test]
    fn an_entry_exists_exactly_only_under_its_own_spelling() {
        let root = tempfile::TempDir::new().expect("a temp dir");
        touch(root.path(), "scripts/ai/Guard.erg");

        assert!(exists_exactly(root.path(), "scripts/ai"));
        assert!(is_file_exactly(root.path(), "scripts/ai/Guard.erg"));
        assert!(!is_file_exactly(root.path(), "scripts/ai/guard.erg"));
        assert!(!exists_exactly(root.path(), "Scripts/ai"));
        assert!(!is_file_exactly(root.path(), "scripts/ai"), "a folder");
        assert!(!exists_exactly(root.path(), "scripts/gone"));
    }
}
