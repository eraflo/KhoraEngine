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

use super::*;
use crate::trust::{Refusals, SecurityReason, Trust};
use std::sync::Arc;
use tempfile::tempdir;

/// Returns the registry of the project at `root`, read as a trusted project's.
fn trusted(root: &std::path::Path) -> AssetIdRegistry {
    AssetIdRegistry::load(&FileGuard::new(root, Trust::Project, Refusals::detached()))
        .expect("the registry reads")
}

/// Makes a registry file at `root` of `len` bytes.
fn registry_file_of(root: &std::path::Path, len: u64) -> PathBuf {
    let path = root.join(REGISTRY_DIR).join(REGISTRY_FILE);
    std::fs::create_dir_all(path.parent().expect("a folder")).expect("the registry's folder");
    std::fs::File::create(&path)
        .and_then(|file| file.set_len(len))
        .expect("the registry file is made");
    path
}

/// **A missing registry file reads as an empty registry**, refused nothing.
#[test]
fn a_missing_registry_reads_as_empty() {
    let dir = tempdir().unwrap();
    let refusals = Arc::new(Refusals::new(None, None));
    let guard = FileGuard::new(dir.path(), Trust::Restricted, Arc::clone(&refusals));
    let registry = AssetIdRegistry::load(&guard).expect("a missing file is no error");
    assert!(registry.is_empty());
    assert!(refusals.quarantined().is_empty());
}

/// **A registry over its cap at the guard's trust is refused**, and the refusal reported.
#[test]
fn a_registry_over_its_cap_is_refused_and_reported() {
    let dir = tempdir().unwrap();
    registry_file_of(dir.path(), (16 << 20) + 1);
    let refusals = Arc::new(Refusals::new(None, None));
    let guard = FileGuard::new(dir.path(), Trust::Restricted, Arc::clone(&refusals));
    match AssetIdRegistry::load(&guard) {
        Err(ReadError::Refused(refusal)) => {
            assert!(matches!(refusal.reason, SecurityReason::TooLarge { .. }));
        }
        other => panic!("the registry is refused: {other:?}"),
    }
    assert!(
        refusals
            .quarantined()
            .iter()
            .any(|refusal| refusal.path.ends_with(REGISTRY_FILE)),
        "{:?}",
        refusals.quarantined()
    );
}

/// **A registry whose file was not read never writes over it**: what it
/// would write would lose every identity the file holds.
#[test]
fn a_registry_not_read_never_writes_over_its_file() {
    let dir = tempdir().unwrap();
    let path = registry_file_of(dir.path(), (16 << 20) + 1);
    let guard = FileGuard::new(dir.path(), Trust::Restricted, Refusals::detached());
    let mut registry = AssetIdRegistry::load_or_unread(&guard);
    assert!(registry.is_empty(), "every path on its default");
    registry.rename("a.png", "b.png");
    assert!(registry.save().is_err(), "the file is not written over");
    assert!(registry.ensure_file().is_ok(), "the file is there");
    assert_eq!(std::fs::metadata(&path).unwrap().len(), (16 << 20) + 1);
}

/// **A registry whose file does not parse never writes over it.**
///
/// A merge left conflict markers in it, and the identities it holds are
/// still the project's, to be mended by hand.
#[test]
fn a_registry_that_does_not_parse_never_writes_over_its_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join(REGISTRY_DIR).join(REGISTRY_FILE);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let conflicted = "<<<<<<< HEAD\n(entries: [(uuid: \"0b3a1f1e-8f4a-5a4e-9b1a-1f2e3d4c5b6a\", \
                      path: \"textures/hero.png\")])\n=======\n(entries: [])\n>>>>>>> theirs\n";
    std::fs::write(&path, conflicted).unwrap();
    let mut registry = trusted(dir.path());

    registry.rename("a.png", "b.png");
    let saved = registry.save();

    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        conflicted,
        "the file was written over: {saved:?}"
    );
}

/// **A registry whose file does not parse is unread, refused for what it holds.**
///
/// The refusal is reported and quarantines the file: read again unchanged,
/// it stays unread; mended, it reads.
#[test]
fn a_registry_that_does_not_parse_is_unread_and_refused() {
    let dir = tempdir().unwrap();
    let path = dir.path().join(REGISTRY_DIR).join(REGISTRY_FILE);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "(entries: [(uuid: ").unwrap();
    let refusals = Arc::new(Refusals::new(None, None));
    let guard = FileGuard::new(dir.path(), Trust::Project, Arc::clone(&refusals));

    let registry = AssetIdRegistry::load_or_unread(&guard);

    assert!(
        !registry.is_read(),
        "a file that does not parse is not read"
    );
    assert!(
        refusals.quarantined().iter().any(|refusal| {
            refusal.path.ends_with(REGISTRY_FILE)
                && matches!(refusal.reason, SecurityReason::Malformed { .. })
        }),
        "{:?}",
        refusals.quarantined()
    );
    assert!(
        !AssetIdRegistry::load_or_unread(&guard).is_read(),
        "read again unchanged, it stays unread"
    );
    std::fs::write(&path, "(entries: [])").unwrap();
    assert!(
        AssetIdRegistry::load(&guard)
            .expect("mended, it reads")
            .is_read(),
        "mended, it is read"
    );
    assert!(
        trusted(tempdir().unwrap().path()).is_read(),
        "a missing file is read"
    );
}

#[test]
fn resolve_falls_back_to_new_v5_without_entry() {
    let dir = tempdir().unwrap();
    let reg = trusted(dir.path());
    assert!(reg.is_empty());
    assert_eq!(
        reg.resolve("textures/wood.png"),
        AssetUUID::new_v5("textures/wood.png"),
        "an unfrozen path must keep the legacy path-derived UUID"
    );
}

#[test]
fn rename_freezes_and_preserves_uuid() {
    let dir = tempdir().unwrap();
    let mut reg = trusted(dir.path());

    let original = reg.resolve("meshes/hero.gltf");
    reg.rename("meshes/hero.gltf", "meshes/protagonist.gltf");

    // The new path resolves to the SAME uuid the old path had — every
    // scene reference (which stored that uuid) keeps resolving.
    assert_eq!(reg.resolve("meshes/protagonist.gltf"), original);
    // A file made at the old path gets a UUID of its own: its default is
    // the moved asset's now.
    assert_ne!(reg.resolve("meshes/hero.gltf"), original);
}

#[test]
fn save_then_load_round_trips() {
    let dir = tempdir().unwrap();
    let mut reg = trusted(dir.path());
    reg.rename("a/one.png", "a/uno.png");
    reg.rename("b/two.glb", "b/dos.glb");
    let frozen_one = reg.resolve("a/uno.png");
    let frozen_two = reg.resolve("b/dos.glb");
    reg.save().unwrap();

    let reloaded = trusted(dir.path());
    assert_eq!(reloaded.len(), 2);
    assert_eq!(reloaded.resolve("a/uno.png"), frozen_one);
    assert_eq!(reloaded.resolve("b/dos.glb"), frozen_two);
}

/// **A path that froze an identity resolves, unfrozen, to what a new file there gets.**
///
/// `b.png` holds `a.png`'s default by a rename; unfrozen, it resolves to
/// its own default, the UUID it has once its entry is removed, and `a.png`
/// still to a derived one, since `b.png` still holds its default.
#[test]
fn a_path_resolves_unfrozen_to_what_a_new_file_there_gets() {
    let dir = tempdir().unwrap();
    let mut reg = trusted(dir.path());
    reg.rename("a.png", "b.png");

    let unfrozen = reg.resolve_unfrozen("b.png");

    assert_eq!(reg.resolve("b.png"), AssetUUID::new_v5("a.png"));
    assert_eq!(unfrozen, AssetUUID::new_v5("b.png"));
    assert_eq!(reg.resolve_unfrozen("a.png"), derived_uuid("a.png", 1));
    reg.remove("b.png");
    assert_eq!(reg.resolve("b.png"), unfrozen);
}

/// **A folder renamed moves the identity of every file inside it**, frozen
/// or on its default, nested folders included.
#[test]
fn rename_folder_moves_every_file_inside_nested_ones_included() {
    let dir = tempdir().unwrap();
    let mut reg = trusted(dir.path());
    reg.rename("textures/old.png", "textures/a.png");
    let frozen = reg.resolve("textures/a.png");

    reg.rename_folder(
        "textures",
        "images",
        ["textures/a.png", "textures/b.png", "textures/deep/c.png"],
    );

    assert_eq!(reg.resolve("images/a.png"), frozen);
    assert_eq!(
        reg.resolve("images/b.png"),
        AssetUUID::new_v5("textures/b.png")
    );
    assert_eq!(
        reg.resolve("images/deep/c.png"),
        AssetUUID::new_v5("textures/deep/c.png")
    );
    assert_ne!(
        reg.resolve("textures/b.png"),
        AssetUUID::new_v5("textures/b.png"),
        "a file made at the old path took the moved asset's identity"
    );
}

/// **A folder renamed leaves a sibling sharing its name as a prefix**:
/// `tex/a.png` is not inside `textures`, nor `textures/a.png` inside `tex`.
#[test]
fn rename_folder_leaves_a_sibling_sharing_its_name_as_a_prefix() {
    let dir = tempdir().unwrap();
    let mut reg = trusted(dir.path());
    reg.rename("textures/old.png", "textures/a.png");
    let kept = reg.resolve("textures/a.png");

    reg.rename_folder("tex", "img", ["tex/a.png", "textures/a.png"]);

    assert_eq!(reg.resolve("textures/a.png"), kept);
    assert_eq!(reg.resolve("img/a.png"), AssetUUID::new_v5("tex/a.png"));
    assert_eq!(reg.len(), 2, "only `tex/a.png` moved");
}

/// **A folder renamed leaves the entry of a file not in it**: a file in the
/// recycle bin, restored at its path, gets its identity back.
#[test]
fn rename_folder_leaves_the_entry_of_a_trashed_file() {
    let dir = tempdir().unwrap();
    let mut reg = trusted(dir.path());
    reg.rename("textures/old.png", "textures/trashed.png");
    let trashed = reg.resolve("textures/trashed.png");

    reg.rename_folder("textures", "images", ["textures/a.png"]);

    assert_eq!(reg.resolve("textures/trashed.png"), trashed);
    assert_eq!(
        reg.resolve("images/a.png"),
        AssetUUID::new_v5("textures/a.png")
    );
}

/// **A folder renamed in case only moves the entry of a trashed file too**:
/// both spellings name one folder, so a restore lands under the new one.
#[test]
fn rename_folder_in_case_moves_the_entry_of_a_trashed_file_too() {
    let dir = tempdir().unwrap();
    let mut reg = trusted(dir.path());
    reg.rename("Tex/old.png", "Tex/trashed.png");
    let trashed = reg.resolve("Tex/trashed.png");
    reg.rename("Texture/old.png", "Texture/kept.png");
    let sibling = reg.resolve("Texture/kept.png");

    reg.rename_folder_in_case("Tex", "tex", ["Tex/a.png"]);

    assert_eq!(reg.resolve("tex/trashed.png"), trashed);
    assert_eq!(reg.resolve("tex/a.png"), AssetUUID::new_v5("Tex/a.png"));
    assert_eq!(reg.resolve("Texture/kept.png"), sibling);
    assert_eq!(reg.len(), 3, "only the paths under `Tex` moved");
}

#[test]
fn remove_drops_entry() {
    let dir = tempdir().unwrap();
    let mut reg = trusted(dir.path());
    reg.rename("x/a.png", "x/b.png");
    assert_eq!(reg.len(), 1);
    reg.remove("x/b.png");
    assert!(reg.is_empty());
}

#[test]
fn a_path_whose_default_another_path_holds_gets_a_uuid_of_its_own() {
    let dir = tempdir().unwrap();
    let mut reg = trusted(dir.path());
    reg.rename("a.png", "b.png");
    // A new file at the old path: the rename moved its default to `b.png`.
    assert_eq!(reg.resolve("b.png"), AssetUUID::new_v5("a.png"));
    assert_eq!(reg.resolve("a.png"), derived_uuid("a.png", 1));
}

#[test]
fn a_derived_uuid_frozen_stays_once_its_holder_is_gone() {
    let dir = tempdir().unwrap();
    let mut reg = trusted(dir.path());
    reg.rename("a.png", "b.png");
    assert!(reg.freeze_derived(["a.png", "b.png"]));
    reg.remove("b.png");
    assert_eq!(reg.resolve("a.png"), derived_uuid("a.png", 1));
}

#[test]
fn a_second_collision_takes_the_next_derived_uuid() {
    let dir = tempdir().unwrap();
    let mut reg = trusted(dir.path());
    reg.rename("a.png", "b.png");
    assert!(reg.freeze_derived(["a.png"]));
    reg.rename("a.png", "c.png");
    assert_eq!(reg.resolve("c.png"), derived_uuid("a.png", 1));
    assert_eq!(reg.resolve("a.png"), derived_uuid("a.png", 2));
}

#[test]
fn paths_on_their_defaults_freeze_nothing() {
    let dir = tempdir().unwrap();
    let mut reg = trusted(dir.path());
    assert!(!reg.freeze_derived(["x.png", "y.png"]));
    assert!(reg.is_empty());
}

#[test]
fn a_rename_over_a_frozen_path_frees_the_uuid_it_held() {
    let dir = tempdir().unwrap();
    let mut reg = trusted(dir.path());
    let held = AssetUUID::new_v5("held");
    reg.freeze("b.png", held);
    reg.rename("a.png", "b.png");
    // `held` is no one's now: a path whose default it is takes it.
    reg.freeze("c.png", AssetUUID::new_v5("c.png"));
    assert_eq!(reg.resolve("b.png"), AssetUUID::new_v5("a.png"));
    assert_eq!(reg.resolve("a.png"), derived_uuid("a.png", 1));
}

#[test]
fn a_derived_uuid_survives_a_save_and_a_load() {
    let dir = tempdir().unwrap();
    let mut reg = trusted(dir.path());
    reg.rename("a.png", "b.png");
    reg.freeze_derived(["a.png"]);
    reg.save().unwrap();
    let loaded = trusted(dir.path());
    assert_eq!(loaded.resolve("a.png"), derived_uuid("a.png", 1));
    assert_eq!(loaded.resolve("b.png"), AssetUUID::new_v5("a.png"));
}

/// **A UUID two paths hold stays held while one of them does.**
///
/// A registry file holding `b.png` and `c.png` frozen to one UUID, as a
/// merge of two renames of `a.png` writes it: once `b.png` is removed,
/// `c.png` still holds that UUID, so a file made at `a.png` gets another,
/// the one a runtime reading the saved file gives it.
#[test]
fn a_uuid_two_paths_hold_stays_held_while_one_of_them_does() {
    let dir = tempdir().unwrap();
    let moved = AssetUUID::new_v5("a.png");
    let mut written = trusted(dir.path());
    written.freeze("b.png", moved);
    written.freeze("c.png", moved);
    written.save().unwrap();

    let mut reg = trusted(dir.path());
    reg.remove("b.png");
    reg.save().unwrap();
    let reloaded = trusted(dir.path());

    assert_eq!(reg.resolve("c.png"), moved);
    assert_ne!(
        reg.resolve("a.png"),
        moved,
        "`a.png` takes the UUID `c.png` still holds"
    );
    assert_eq!(
        reg.resolve("a.png"),
        reloaded.resolve("a.png"),
        "the editor and a runtime reading the saved file disagree"
    );
}

/// **A derived UUID is no other path's default.**
///
/// Once `a.png` is renamed to `b.png`, a file made at `a.png` derives its
/// UUID from `a.png`, a NUL and a count; a file literally named `a.png#1`
/// keeps its own default, before and after the editor freezes what is
/// derived.
#[test]
fn a_derived_uuid_is_no_other_paths_default() {
    let dir = tempdir().unwrap();
    let mut reg = trusted(dir.path());
    let own = reg.resolve("a.png#1");
    reg.rename("a.png", "b.png");

    assert_ne!(
        reg.resolve("a.png"),
        reg.resolve("a.png#1"),
        "two files, one UUID"
    );
    reg.freeze_derived(["a.png", "a.png#1", "b.png"]);
    assert_eq!(
        reg.resolve("a.png#1"),
        own,
        "an asset no rename touched changed its UUID"
    );
    assert_ne!(reg.resolve("a.png"), own);
}

/// Writes `text` as the registry file of the project at `root`, its folder made.
fn write_registry(root: &std::path::Path, text: &str) -> PathBuf {
    let path = root.join(REGISTRY_DIR).join(REGISTRY_FILE);
    std::fs::create_dir_all(path.parent().expect("a folder")).expect("the registry's folder");
    std::fs::write(&path, text).expect("the registry file is written");
    path
}

/// The text of a registry file freezing `path` to `uuid`.
fn frozen_text(path: &str, uuid: AssetUUID) -> String {
    let file = RegistryFile {
        entries: vec![RegistryEntry {
            uuid,
            path: path.to_owned(),
        }],
    };
    ron::ser::to_string(&file).expect("a registry file serialises")
}

/// **A registry whose file was mended reads it once it follows it.**
///
/// Unread for conflict markers, the registry reads the mended file at the
/// next [`AssetIdRegistry::follow`], with no new load.
#[test]
fn a_registry_mended_reads_once_it_follows_its_file() {
    let dir = tempdir().unwrap();
    write_registry(
        dir.path(),
        "<<<<<<< HEAD\n(entries: [])\n=======\n>>>>>>> theirs\n",
    );
    let mut registry = AssetIdRegistry::load_or_unread(&FileGuard::new(
        dir.path(),
        Trust::Project,
        Refusals::detached(),
    ));
    assert!(!registry.is_read(), "the premise: it does not parse");
    let moved = AssetUUID::new_v5("textures/a.png");
    write_registry(dir.path(), &frozen_text("textures/b.png", moved));

    assert!(registry.follow(), "the mended file is read");
    assert!(registry.is_read());
    assert_eq!(registry.resolve("textures/b.png"), moved);
}

/// **A registry follows a change made to its file outside it.**
///
/// A pull brings in an identity: the registry holds it once it follows
/// the file; followed again unchanged, it reads nothing new.
#[test]
fn a_registry_follows_a_change_made_to_its_file_outside_it() {
    let dir = tempdir().unwrap();
    let mut registry = trusted(dir.path());
    registry.ensure_file().unwrap();
    let moved = AssetUUID::new_v5("textures/a.png");
    write_registry(dir.path(), &frozen_text("textures/b.png", moved));

    assert!(registry.follow(), "the change is read");
    assert_eq!(registry.resolve("textures/b.png"), moved);
    assert!(!registry.follow(), "unchanged, nothing is read again");
    assert_eq!(registry.resolve("textures/b.png"), moved);
}

/// **A registry whose file stops parsing is unread once it follows it, refused for what it holds.**
#[test]
fn a_registry_whose_file_stops_parsing_is_unread_once_it_follows_it() {
    let dir = tempdir().unwrap();
    let refusals = Arc::new(Refusals::new(None, None));
    let guard = FileGuard::new(dir.path(), Trust::Project, Arc::clone(&refusals));
    let mut registry = AssetIdRegistry::load(&guard).unwrap();
    registry.rename("a.png", "b.png");
    registry.save().unwrap();
    let conflicted = "<<<<<<< HEAD\n(entries: [])\n=======\n>>>>>>> theirs\n";
    let path = write_registry(dir.path(), conflicted);

    assert!(registry.follow(), "what it holds changed");
    assert!(
        !registry.is_read(),
        "a file that does not parse is not read"
    );
    assert!(
        refusals.quarantined().iter().any(|refusal| {
            refusal.path.ends_with(REGISTRY_FILE)
                && matches!(refusal.reason, SecurityReason::Malformed { .. })
        }),
        "{:?}",
        refusals.quarantined()
    );
    assert_eq!(registry.resolve("b.png"), AssetUUID::new_v5("b.png"));
    assert!(registry.save().is_err());
    assert_eq!(std::fs::read_to_string(path).unwrap(), conflicted);
}

/// **A save never writes over a change made to the file since it was read.**
///
/// The file holds an identity a pull brought in after the registry read
/// it: the registry's save is refused, and the file keeps that identity.
#[test]
fn a_save_never_writes_over_a_change_made_since_the_file_was_read() {
    let dir = tempdir().unwrap();
    let mut registry = trusted(dir.path());
    registry.ensure_file().unwrap();
    let pulled = frozen_text("textures/b.png", AssetUUID::new_v5("textures/a.png"));
    let path = write_registry(dir.path(), &pulled);

    registry.rename("c.png", "d.png");
    let saved = registry.save();

    assert!(saved.is_err(), "the save wrote over the change");
    assert_eq!(std::fs::read_to_string(path).unwrap(), pulled);
}

/// **A registry saves again over the file it wrote itself.**
#[test]
fn a_registry_saves_again_over_the_file_it_wrote() {
    let dir = tempdir().unwrap();
    let mut registry = trusted(dir.path());
    registry.rename("a.png", "b.png");
    registry.save().unwrap();
    registry.rename("c.png", "d.png");

    registry.save().expect("its own write is what it read");

    assert!(!registry.follow(), "its own write is no change");
    assert_eq!(trusted(dir.path()).len(), 2);
}

/// **A registry whose file is deleted holds no identity once it follows it, and saves it anew.**
///
/// The file on disk is the truth: gone, it froze nothing, and the next
/// save makes it again rather than being refused as changed.
#[test]
fn a_registry_whose_file_is_deleted_holds_none_once_it_follows_and_saves_anew() {
    let dir = tempdir().unwrap();
    let mut registry = trusted(dir.path());
    registry.rename("a.png", "b.png");
    registry.save().unwrap();
    std::fs::remove_file(registry.file_path()).unwrap();

    assert!(registry.follow(), "the deletion is a change");
    assert!(registry.is_read(), "a missing file is read");
    assert_eq!(registry.resolve("b.png"), AssetUUID::new_v5("b.png"));
    registry.rename("c.png", "d.png");
    registry.save().expect("a deleted file is made again");

    let read = trusted(dir.path());
    assert_eq!(read.len(), 1, "only what was frozen since the deletion");
    assert_eq!(read.resolve("d.png"), AssetUUID::new_v5("c.png"));
}

/// **Two registries saving one file never lose what either saved.**
///
/// Two editors open on one project each freeze identities, following the
/// file and saving again when a save is refused: every save that returned
/// `Ok` is in the file at the end.
#[test]
fn two_registries_saving_one_file_never_lose_what_either_saved() {
    const SAVES: usize = 40;
    let dir = tempdir().unwrap();
    trusted(dir.path()).ensure_file().unwrap();
    let editor = |name: &str| {
        let mut registry = trusted(dir.path());
        for i in 0..SAVES {
            let path = format!("{name}/{i}.png");
            loop {
                registry.follow();
                registry.freeze(&path, AssetUUID::new_v5(&format!("{path}#moved")));
                if registry.save().is_ok() {
                    break;
                }
            }
        }
    };
    std::thread::scope(|scope| {
        scope.spawn(|| editor("left"));
        scope.spawn(|| editor("right"));
    });

    let read = trusted(dir.path());
    let lost: Vec<String> = ["left", "right"]
        .into_iter()
        .flat_map(|name| (0..SAVES).map(move |i| format!("{name}/{i}.png")))
        .filter(|path| read.resolve(path) != AssetUUID::new_v5(&format!("{path}#moved")))
        .collect();
    assert!(
        lost.is_empty(),
        "{} saved identities were written over: {:?}",
        lost.len(),
        &lost[..lost.len().min(5)]
    );
}

/// **Only an asset is frozen**: a renamed `.bin` or README keeps no
/// identity, its old entry dropped, and one in the file is not read.
#[test]
fn a_file_that_is_no_asset_is_never_frozen() {
    let dir = tempfile::tempdir().unwrap();
    let mut reg = trusted(dir.path());
    reg.freeze("models/a.bin", AssetUUID::new_v5("frozen"));
    reg.rename("docs/README.md", "docs/NOTES.md");
    assert!(reg.is_empty(), "nothing is frozen for a file of no asset");

    reg.rename("textures/a.png", "textures/a.bin");
    assert!(
        reg.is_empty(),
        "an asset moved to no asset's path keeps no identity"
    );

    // A file naming a `.bin`: written as an asset's entry, its path changed.
    let uuid = AssetUUID::new_v5("textures/old.png");
    reg.freeze("textures/new.png", uuid);
    reg.freeze("textures/c.png", AssetUUID::new_v5("textures/old_c.png"));
    reg.save().unwrap();
    let text = std::fs::read_to_string(reg.file_path()).unwrap();
    std::fs::write(
        reg.file_path(),
        text.replace("textures/c.png", "models/a.bin"),
    )
    .unwrap();
    let read = trusted(dir.path());
    assert_eq!(read.len(), 1, "only the asset's entry is read");
    assert_eq!(read.resolve("textures/new.png"), uuid);
    assert_eq!(
        read.resolve("models/a.bin"),
        AssetUUID::new_v5("models/a.bin")
    );
}
