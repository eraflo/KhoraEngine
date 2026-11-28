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

//! What the identity registry keeps of an asset across a rename, a trash, a write and a pull.

use super::*;

/// Renames `textures/a.png`, made in `pvfs`, to `textures/b.png`, then trashes it into `bin`; returns the file in the bin.
fn renamed_then_trashed(pvfs: &mut ProjectVfs, bin: &Path) -> Result<PathBuf> {
    pvfs.create_asset("textures/a.png", b"first")?;
    pvfs.rename_asset("textures/a.png", "textures/b.png")?;
    std::fs::create_dir_all(bin)?;
    let binned = bin.join("b.png");
    pvfs.trash_with("textures/b.png", |abs| Ok(std::fs::rename(abs, &binned)?))?;
    Ok(binned)
}

/// **A rename whose identity the registry cannot keep says so.**
///
/// The registry's file was refused, so the UUID the rename freezes is
/// never saved: the moved asset's references would dangle at the next open.
#[test]
fn a_rename_whose_identity_cannot_be_kept_says_so() -> Result<()> {
    let project = TempProject::new("rename-unkept")?;
    project.refuse_registry()?;
    project.put("textures/a.png", b"first")?;
    let mut pvfs = project.open()?;

    let renamed = pvfs.rename_asset("textures/a.png", "textures/b.png");

    assert!(
        renamed.is_err(),
        "the rename reports success though its identity is not saved"
    );
    Ok(())
}

/// **What would freeze or drop an identity is refused while the registry is unread.**
///
/// A move, a trash and a new file, like a rename, change the registry, whose
/// file was refused: a new file must drop the identity a trashed file left
/// at its path, or take it once the registry reads. Each is refused before
/// anything moves or is made, and says why.
#[test]
fn what_would_change_an_identity_is_refused_while_the_registry_is_unread() -> Result<()> {
    let project = TempProject::new("unread-refuses")?;
    let registry = project.refuse_registry()?;
    let file = project.put("textures/a.png", b"first")?;
    let mut pvfs = project.open()?;

    let renamed = pvfs
        .rename_asset("textures/a.png", "textures/b.png")
        .map(drop);
    let moved = pvfs.move_asset("textures/a.png", "elsewhere").map(drop);
    let trashed = pvfs.delete_to_trash("textures/a.png").map(drop);
    let created = pvfs.create_asset("scripts/Player.erg", b"behavior Player { }");

    for (what, outcome) in [
        ("rename", renamed),
        ("move", moved),
        ("trash", trashed),
        ("create", created),
    ] {
        refused_for_the_unread_registry(what, outcome);
    }
    assert_eq!(std::fs::read(&file)?, b"first", "the file never moved");
    assert!(
        !project.0.join("assets").join("scripts").exists(),
        "the refused new file made its folder"
    );
    assert_eq!(
        std::fs::metadata(&registry)?.len(),
        (64 << 20) + 1,
        "the registry is not written over"
    );
    Ok(())
}

/// **A duplicate and a save-as to a new path are refused while the registry is unread.**
///
/// Each makes a new file, which could not drop the identity a trashed file
/// left at its path: each is refused before the file is made, and says why.
#[test]
fn a_duplicate_and_a_save_as_to_a_new_path_are_refused_while_the_registry_is_unread() -> Result<()>
{
    let project = TempProject::new("unread-refuses-new-files")?;
    let registry = project.refuse_registry()?;
    project.put("textures/c.png", b"first")?;
    let mut pvfs = project.open()?;

    let duplicated = pvfs.duplicate_asset("textures/c.png");
    let saved_as = pvfs.write_asset(Path::new("scenes/level.kscene"), b"scene bytes");

    refused_for_the_unread_registry("duplicate", duplicated);
    refused_for_the_unread_registry("save-as", saved_as);
    let assets = project.0.join("assets");
    assert!(
        !assets.join("textures").join("c copy.png").exists(),
        "the refused duplicate was made"
    );
    assert!(
        !assets.join("scenes").join("level.kscene").exists(),
        "the refused save-as was written"
    );
    assert_eq!(
        std::fs::metadata(&registry)?.len(),
        (64 << 20) + 1,
        "the registry is not written over"
    );
    Ok(())
}

/// **A save over an existing file works while the registry is unread**: it
/// is the same asset, saved again, and changes no identity.
#[test]
fn a_save_over_an_existing_file_works_while_the_registry_is_unread() -> Result<()> {
    let project = TempProject::new("unread-saves-over")?;
    let registry = project.refuse_registry()?;
    let scene = project.put("scenes/level.kscene", b"first")?;
    let mut pvfs = project.open()?;

    pvfs.write_asset(Path::new("scenes/level.kscene"), b"second")?;

    assert_eq!(std::fs::read(&scene)?, b"second");
    assert_eq!(
        std::fs::metadata(&registry)?.len(),
        (64 << 20) + 1,
        "the registry is not written over"
    );
    Ok(())
}

/// **A trust change reads the identity registry again.**
///
/// A registry over the strict cap is refused while the project is
/// restricted, so a rename is refused; once the project is trusted and its
/// reads made again, the registry reads, and a rename keeps the asset's identity.
#[test]
fn a_trust_change_reads_the_identity_registry_again() -> Result<()> {
    use khora_sdk::trust::{ProjectTrust, ProjectTrustState, Refusals, Trust};

    let project = TempProject::new("trust-rereads")?;
    project.put("textures/a.png", b"first")?;
    let registry = AssetIdRegistry::unread(&project.0).file_path();
    std::fs::create_dir_all(registry.parent().expect("a folder"))?;
    let mut padded = b"(entries: [])".to_vec();
    padded.resize((16 << 20) + 1, b' ');
    std::fs::write(&registry, padded)?;
    let refusals = Arc::new(Refusals::new(None, None));
    let trust = Arc::new(ProjectTrustState::new(ProjectTrust::Restricted));
    trust.set_for(&project.0, ProjectTrust::Restricted);
    let mut pvfs = ProjectVfs::over_service(
        project.0.clone(),
        Arc::new(Mutex::new(AssetService::empty())),
        Arc::new(AssetWatch::default()),
        ProjectReads::new(Arc::clone(&refusals), Arc::clone(&trust)),
    )?;
    assert!(
        pvfs.rename_asset("textures/a.png", "textures/b.png")
            .is_err(),
        "the premise: the registry is refused while restricted"
    );

    trust.set_for(&project.0, ProjectTrust::Trusted);
    refusals.release_strict_under(Trust::Restricted, &project.0);
    pvfs.reread_at_its_trust();
    pvfs.rename_asset("textures/a.png", "textures/b.png")?;

    let saved = AssetIdRegistry::load(&khora_sdk::trust::FileGuard::new(
        &project.0,
        Trust::Project,
        Refusals::detached(),
    ))
    .map_err(|error| anyhow::anyhow!("the registry reads: {error}"))?;
    assert_eq!(
        saved.resolve("textures/b.png"),
        AssetUUID::new_v5("textures/a.png"),
        "the rename kept the asset's identity"
    );
    Ok(())
}

/// **A registry mended while the project is open reads at the next rename.**
///
/// A merge left conflict markers in the registry, so a rename is refused and
/// told to mend the file and try again; mended, the same rename keeps the
/// asset's identity, with no reopen.
#[test]
fn a_registry_mended_while_the_project_is_open_reads_at_the_next_rename() -> Result<()> {
    use khora_sdk::trust::{ProjectTrust, ProjectTrustState, Refusals, Trust};

    let project = TempProject::new("mended-registry")?;
    project.put("textures/a.png", b"first")?;
    let registry = AssetIdRegistry::unread(&project.0).file_path();
    std::fs::create_dir_all(registry.parent().expect("a folder"))?;
    std::fs::write(
        &registry,
        "<<<<<<< HEAD\n(entries: [])\n=======\n>>>>>>> theirs\n",
    )?;
    let refusals = Arc::new(Refusals::new(None, None));
    let trust = Arc::new(ProjectTrustState::new(ProjectTrust::Trusted));
    trust.set_for(&project.0, ProjectTrust::Trusted);
    let mut pvfs = ProjectVfs::over_service(
        project.0.clone(),
        Arc::new(Mutex::new(AssetService::empty())),
        Arc::new(AssetWatch::default()),
        ProjectReads::new(Arc::clone(&refusals), Arc::clone(&trust)),
    )?;
    let refused = pvfs.rename_asset("textures/a.png", "textures/b.png");
    assert!(
        refused.is_err_and(|error| format!("{error:#}").contains("Mend the file")),
        "the premise: a registry that does not parse refuses a rename"
    );

    std::fs::write(&registry, "(entries: [])")?;
    let renamed = pvfs.rename_asset("textures/a.png", "textures/b.png");

    assert!(
        renamed.is_ok(),
        "mended, the rename is refused: {renamed:?}"
    );
    let saved = AssetIdRegistry::load(&khora_sdk::trust::FileGuard::new(
        &project.0,
        Trust::Project,
        Refusals::detached(),
    ))
    .map_err(|error| anyhow::anyhow!("the registry reads: {error}"))?;
    assert_eq!(
        saved.resolve("textures/b.png"),
        AssetUUID::new_v5("textures/a.png"),
        "the rename kept the asset's identity"
    );
    Ok(())
}

/// **A registry that stops parsing while the project is open is never written over.**
///
/// A merge run while the editor is open leaves conflict markers in a registry
/// the editor read before: the next rename must not write the editor's copy
/// over the file, losing the identities the merge brought in.
#[test]
fn a_registry_that_stops_parsing_while_the_project_is_open_is_never_written_over() -> Result<()> {
    let project = TempProject::new("conflicted-while-open")?;
    let mut pvfs = project.open()?;
    pvfs.create_asset("textures/a.png", b"first")?;
    let registry = AssetIdRegistry::unread(&project.0).file_path();
    let conflicted = "<<<<<<< HEAD\n(entries: [])\n=======\n(entries: [(uuid: \
                      \"0b3a1f1e-8f4a-5a4e-9b1a-1f2e3d4c5b6a\", path: \"textures/hero.png\")])\n\
                      >>>>>>> theirs\n";
    std::fs::write(&registry, conflicted)?;

    let renamed = pvfs.rename_asset("textures/a.png", "textures/b.png");

    assert_eq!(
        std::fs::read_to_string(&registry)?,
        conflicted,
        "the rename wrote over the conflicted registry: {renamed:?}"
    );
    Ok(())
}

/// **A rename whose registry cannot be saved moves the file back**, and says so.
#[test]
fn a_rename_whose_registry_cannot_be_saved_moves_the_file_back() -> Result<()> {
    let project = TempProject::new("rename-save-fails")?;
    let mut pvfs = project.open()?;
    pvfs.create_asset("textures/a.png", b"first")?;
    let registry = AssetIdRegistry::unread(&project.0).file_path();
    let set_read_only = |read_only: bool| -> Result<()> {
        let mut permissions = std::fs::metadata(&registry)?.permissions();
        permissions.set_readonly(read_only);
        Ok(std::fs::set_permissions(&registry, permissions)?)
    };
    set_read_only(true)?;

    let renamed = pvfs.rename_asset("textures/a.png", "textures/b.png");
    set_read_only(false)?;

    let error = format!("{:#}", renamed.expect_err("the registry is read-only"));
    assert!(error.contains("left where it was"), "{error}");
    let textures = project.0.join("assets").join("textures");
    assert_eq!(std::fs::read(textures.join("a.png"))?, b"first");
    assert!(!textures.join("b.png").exists(), "the file is moved back");
    assert_eq!(
        pvfs.resolve_uuid("textures/a.png"),
        AssetUUID::new_v5("textures/a.png"),
        "the registry is as it was"
    );
    Ok(())
}

/// **A trash never writes the identity registry.**
///
/// `textures/b.png` holds a frozen identity, its rename's; the registry is
/// read-only, and the trash sends the file all the same: the file keeps its
/// identity in the registry, for a restore at its path.
#[test]
fn a_trash_never_writes_the_identity_registry() -> Result<()> {
    let project = TempProject::new("trash-writes-nothing")?;
    let mut pvfs = project.open()?;
    pvfs.create_asset("textures/a.png", b"first")?;
    pvfs.rename_asset("textures/a.png", "textures/b.png")?;
    let registry = AssetIdRegistry::unread(&project.0).file_path();
    let saved_before = std::fs::read(&registry)?;
    let set_read_only = |read_only: bool| -> Result<()> {
        let mut permissions = std::fs::metadata(&registry)?.permissions();
        permissions.set_readonly(read_only);
        Ok(std::fs::set_permissions(&registry, permissions)?)
    };
    set_read_only(true)?;

    let trashed = pvfs.trash_with("textures/b.png", |abs| Ok(std::fs::remove_file(abs)?));
    set_read_only(false)?;

    trashed?;
    let file = project.0.join("assets").join("textures").join("b.png");
    assert!(!file.exists(), "the file is sent");
    assert_eq!(
        std::fs::read(&registry)?,
        saved_before,
        "the registry was written"
    );
    assert_eq!(
        pvfs.resolve_uuid("textures/b.png"),
        AssetUUID::new_v5("textures/a.png"),
        "the identity is kept"
    );
    Ok(())
}

/// **A trash that fails to send the file leaves the file and its identity.**
#[test]
fn a_trash_that_fails_to_send_the_file_leaves_the_file_and_its_identity() -> Result<()> {
    let project = TempProject::new("trash-send-fails")?;
    let mut pvfs = project.open()?;
    pvfs.create_asset("textures/a.png", b"first")?;
    pvfs.rename_asset("textures/a.png", "textures/b.png")?;

    let trashed = pvfs.trash_with("textures/b.png", |_| anyhow::bail!("the bin is full"));

    let error = format!("{:#}", trashed.expect_err("the bin refused it"));
    assert!(error.contains("the bin is full"), "{error}");
    let file = project.0.join("assets").join("textures").join("b.png");
    assert_eq!(std::fs::read(file)?, b"first", "the file stays");
    assert_eq!(
        saved_registry(&project.0).resolve("textures/b.png"),
        AssetUUID::new_v5("textures/a.png"),
        "the saved registry lost the identity of a file still there"
    );
    assert_eq!(
        pvfs.resolve_uuid("textures/b.png"),
        AssetUUID::new_v5("textures/a.png")
    );
    Ok(())
}

/// **A trash cut short before the file goes leaves the file its identity.**
///
/// The editor stops (a crash) between the registry's save and the file's
/// send: the file is still in the project, so a reopened project gives it the
/// frozen identity its references stored, not its path's.
#[test]
fn a_trash_cut_short_before_the_file_goes_leaves_the_file_its_identity() -> Result<()> {
    let project = TempProject::new("trash-cut-short")?;
    let mut pvfs = project.open()?;
    pvfs.create_asset("textures/a.png", b"first")?;
    pvfs.rename_asset("textures/a.png", "textures/b.png")?;

    let cut_short = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pvfs.trash_with("textures/b.png", |_| panic!("the editor stops"))
    }));
    assert!(cut_short.is_err(), "the premise: the trash was cut short");
    drop(pvfs);

    let file = project.0.join("assets").join("textures").join("b.png");
    assert_eq!(
        std::fs::read(file)?,
        b"first",
        "the premise: the file stays"
    );
    assert_eq!(
        project.open()?.resolve_uuid("textures/b.png"),
        AssetUUID::new_v5("textures/a.png"),
        "the file still there lost the identity its references stored"
    );
    Ok(())
}

/// **A trash names what it sent: the folder's key, and the identity of every file it held.**
#[test]
fn a_trash_names_the_key_and_the_identities_it_sent() -> Result<()> {
    let project = TempProject::new("trash-names")?;
    let mut pvfs = project.open()?;
    pvfs.create_asset("textures/a.png", b"a")?;
    pvfs.create_asset("textures/hd/b.png", b"b")?;
    pvfs.rename_asset("textures/a.png", "textures/c.png")?;
    let bin = project.0.join("bin");
    std::fs::create_dir_all(&bin)?;

    let trashed = pvfs.trash_with("textures", |abs| Ok(std::fs::rename(abs, bin.join("t"))?))?;

    let KeyChange::Trashed { key, identities } = &trashed else {
        panic!("a trash said it moved: {trashed:?}");
    };
    assert_eq!(key, "textures");
    assert!(
        trashed.trashed(AssetUUID::new_v5("textures/a.png")),
        "{identities:?}"
    );
    assert!(
        trashed.trashed(AssetUUID::new_v5("textures/hd/b.png")),
        "{identities:?}"
    );
    assert_eq!(identities.len(), 2, "{identities:?}");
    Ok(())
}

/// **What waits for the edited scene follows a key change**: a reveal of a file moved names its new path, a material trashed is assigned to no one.
#[test]
fn follow_key_change_rewrites_or_drops_what_waits() -> Result<()> {
    let project = TempProject::new("follow-key-change")?;
    let mut pvfs = project.open()?;
    let gone = AssetUUID::new_v5("materials/a.kmat");
    pvfs.hold(vec![
        AfterIndex::Reveal("scenes/level.kscene".to_owned()),
        AfterIndex::AssignMaterial {
            entity: khora_sdk::prelude::ecs::EntityId {
                index: 0,
                generation: 0,
            },
            persistent: None,
            uuid: gone,
            saved: String::new(),
            choice: 0,
            played: false,
        },
    ]);

    pvfs.follow_key_change(&KeyChange::Moved {
        from: "scenes".to_owned(),
        to: "levels".to_owned(),
    });
    pvfs.follow_key_change(&KeyChange::Trashed {
        key: "materials".to_owned(),
        identities: vec![gone],
    });

    assert_eq!(
        pvfs.release(),
        vec![AfterIndex::Reveal("levels/level.kscene".to_owned())]
    );
    Ok(())
}

/// **A trash keeps the identity of the asset it sent, in the saved registry.**
#[test]
fn a_trash_keeps_the_identity_of_the_asset_it_sent() -> Result<()> {
    let project = TempProject::new("trash-keeps")?;
    let mut pvfs = project.open()?;

    renamed_then_trashed(&mut pvfs, &project.0.join("bin"))?;

    let textures = project.0.join("assets").join("textures");
    assert!(!textures.join("b.png").exists(), "the file is sent");
    assert_eq!(
        saved_registry(&project.0).resolve("textures/b.png"),
        AssetUUID::new_v5("textures/a.png"),
        "the trashed file's identity is dropped"
    );
    Ok(())
}

/// **A trashed renamed asset restored at its path keeps its identity.**
///
/// Its references stored the identity its rename froze: once the file is
/// back from the recycle bin, the index lists it under that identity again.
#[test]
fn a_trashed_renamed_asset_restored_at_its_path_keeps_its_identity() -> Result<()> {
    let project = TempProject::new("trash-restored")?;
    let mut pvfs = project.open()?;
    let binned = renamed_then_trashed(&mut pvfs, &project.0.join("bin"))?;

    let restored = project.0.join("assets").join("textures").join("b.png");
    std::fs::rename(&binned, &restored)?;
    pvfs.reindex()?;

    let moved = AssetUUID::new_v5("textures/a.png");
    assert_eq!(pvfs.resolve_uuid("textures/b.png"), moved);
    let listed = crate::hot_reload::collect_asset_entries(&pvfs);
    assert!(
        listed
            .iter()
            .any(|entry| entry.source_path == "textures/b.png" && entry.uuid == moved),
        "the restored file is not listed under its identity: {listed:?}"
    );
    assert_eq!(project.open()?.resolve_uuid("textures/b.png"), moved);
    Ok(())
}

/// **A new file made at a trashed file's path gets a new identity.**
///
/// The trashed `textures/b.png` holds `textures/a.png`'s identity, which its
/// references stored: a file made at its path is another asset, and must
/// not take it. Its stale entry is dropped, and the saved registry says so.
#[test]
fn a_new_file_made_at_a_trashed_files_path_gets_a_new_identity() -> Result<()> {
    let project = TempProject::new("trash-path-made")?;
    let mut pvfs = project.open()?;
    renamed_then_trashed(&mut pvfs, &project.0.join("bin"))?;

    pvfs.create_asset("textures/b.png", b"second")?;

    let moved = AssetUUID::new_v5("textures/a.png");
    assert_eq!(
        pvfs.resolve_uuid("textures/b.png"),
        AssetUUID::new_v5("textures/b.png"),
        "the new file took the trashed file's identity"
    );
    assert_ne!(saved_registry(&project.0).resolve("textures/b.png"), moved);
    Ok(())
}

/// **A file written anew at a trashed file's path gets a new identity**:
/// a scene, a material or a prefab saved there, whose id is known before
/// the write, is another asset too.
#[test]
fn a_file_written_anew_at_a_trashed_files_path_gets_a_new_identity() -> Result<()> {
    let project = TempProject::new("trash-path-written")?;
    let mut pvfs = project.open()?;
    renamed_then_trashed(&mut pvfs, &project.0.join("bin"))?;
    let moved = AssetUUID::new_v5("textures/a.png");

    let written = pvfs.written_asset_id("textures/b.png");
    pvfs.write_asset(Path::new("textures/b.png"), b"second")?;

    assert_ne!(
        written, moved,
        "the id known before the write is the trashed file's"
    );
    assert_eq!(pvfs.resolve_uuid("textures/b.png"), written);
    assert_eq!(
        saved_registry(&project.0).resolve("textures/b.png"),
        written
    );
    Ok(())
}

/// **A file written over a renamed asset keeps the asset's identity**: it is
/// the same asset, saved again.
#[test]
fn a_file_written_over_a_renamed_asset_keeps_its_identity() -> Result<()> {
    let project = TempProject::new("written-over")?;
    let mut pvfs = project.open()?;
    pvfs.create_asset("textures/a.png", b"first")?;
    pvfs.rename_asset("textures/a.png", "textures/b.png")?;

    pvfs.write_asset(Path::new("textures/b.png"), b"second")?;

    let moved = AssetUUID::new_v5("textures/a.png");
    assert_eq!(pvfs.written_asset_id("textures/b.png"), moved);
    assert_eq!(pvfs.resolve_uuid("textures/b.png"), moved);
    assert_eq!(saved_registry(&project.0).resolve("textures/b.png"), moved);
    Ok(())
}

/// **The id known before a new file is written is the one it gets, after a pull changed the registry.**
///
/// A pull renamed `textures/c.png` to `textures/z.png` in another clone: `z`
/// holds `c`'s default now, so a file made at `textures/c.png` gets another.
/// The id a prefab save writes into the file is the one the file is indexed by.
#[test]
fn the_id_known_before_a_new_file_is_written_is_its_id_after_a_pull() -> Result<()> {
    let project = TempProject::new("written-id-after-pull")?;
    let mut pvfs = project.open()?;
    let mut pulled = saved_registry(&project.0);
    pulled.rename("textures/c.png", "textures/z.png");
    pulled.save()?;
    project.put("textures/z.png", b"theirs")?;

    let written = pvfs.written_asset_id("textures/c.png");
    pvfs.write_asset(Path::new("textures/c.png"), b"mine")?;
    pvfs.rebuild_index()?;

    assert_eq!(
        pvfs.resolve_uuid("textures/c.png"),
        written,
        "the file is indexed under another id than the one known before its write"
    );
    Ok(())
}

/// **A folder renamed keeps the identity of every asset inside it.**
///
/// The asset browser renames a folder through the same rename as a file:
/// each asset in it moves, and its references stored its identity.
#[test]
fn a_folder_renamed_keeps_the_identity_of_every_asset_inside() -> Result<()> {
    let project = TempProject::new("folder-renamed")?;
    let mut pvfs = project.open()?;
    pvfs.create_asset("textures/a.png", b"first")?;

    pvfs.rename_asset("textures", "images")?;

    let kept = AssetUUID::new_v5("textures/a.png");
    assert_eq!(
        pvfs.resolve_uuid("images/a.png"),
        kept,
        "the asset moved with its folder lost its identity"
    );
    let listed = crate::hot_reload::collect_asset_entries(&pvfs);
    assert!(
        listed
            .iter()
            .any(|entry| entry.source_path == "images/a.png" && entry.uuid == kept),
        "{listed:?}"
    );
    Ok(())
}

/// **A duplicate made at a trashed file's path gets a new identity.**
///
/// `textures/c copy.png`, renamed from `textures/x.png`, is trashed: the
/// duplicate of `textures/c.png` takes its path, and is another asset.
#[test]
fn a_duplicate_made_at_a_trashed_files_path_gets_a_new_identity() -> Result<()> {
    let project = TempProject::new("trash-path-duplicated")?;
    let mut pvfs = project.open()?;
    pvfs.create_asset("textures/x.png", b"first")?;
    pvfs.rename_asset("textures/x.png", "textures/c copy.png")?;
    pvfs.trash_with("textures/c copy.png", |abs| Ok(std::fs::remove_file(abs)?))?;
    pvfs.create_asset("textures/c.png", b"second")?;

    let copy = pvfs.duplicate_asset("textures/c.png")?;

    assert_eq!(copy, "textures/c copy.png");
    assert_eq!(
        pvfs.resolve_uuid(&copy),
        AssetUUID::new_v5("textures/c copy.png"),
        "the duplicate took the trashed file's identity"
    );
    assert_eq!(
        saved_registry(&project.0).resolve(&copy),
        AssetUUID::new_v5("textures/c copy.png")
    );
    Ok(())
}

/// **A file made at a renamed asset's old path is an asset of its own.**
///
/// The rename gave the old path's default UUID to the new path; the new
/// file gets another, which the editor freezes, so it stays that file's
/// once the renamed asset is gone.
#[test]
fn a_file_made_at_a_renamed_assets_old_path_gets_a_frozen_uuid_of_its_own() -> Result<()> {
    let project = TempProject::new("old-path")?;
    let mut pvfs = project.open()?;
    pvfs.create_asset("textures/a.png", b"first")?;
    pvfs.rename_asset("textures/a.png", "textures/b.png")?;

    pvfs.create_asset("textures/a.png", b"second")?;

    let listed = crate::hot_reload::collect_asset_entries(&pvfs);
    let uuid_of = |path: &str| {
        listed
            .iter()
            .find(|entry| entry.source_path == path)
            .map(|entry| entry.uuid)
    };
    let (new, moved) = (uuid_of("textures/a.png"), uuid_of("textures/b.png"));
    assert!(
        new.is_some() && moved.is_some(),
        "both are listed: {listed:?}"
    );
    assert_ne!(new, moved, "two assets, two UUIDs");
    let saved = AssetIdRegistry::load_or_unread(&khora_sdk::trust::FileGuard::new(
        &project.0,
        khora_sdk::trust::Trust::Project,
        khora_sdk::trust::Refusals::detached(),
    ));
    assert_eq!(Some(saved.resolve("textures/a.png")), new);
    assert_eq!(saved.len(), 2, "the new file's UUID is frozen");
    Ok(())
}

/// **A folder renamed in another case only, where file names ignore case, keeps every asset inside.**
///
/// `Tex` renamed `tex` is the same folder: it is no destination that exists.
#[test]
fn a_folder_renamed_in_another_case_only_keeps_its_assets() -> Result<()> {
    let project = TempProject::new("folder-case-rename")?;
    if !ignores_case(&project) {
        return Ok(()); // The file system tells case apart: two folders.
    }
    project.put("Tex/a.png", b"first")?;
    let mut pvfs = project.open()?;

    let renamed = pvfs.rename_asset("Tex", "tex");

    assert!(
        renamed.is_ok(),
        "the case-only rename is refused: {renamed:?}"
    );
    let names: Vec<String> = std::fs::read_dir(project.0.join("assets"))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().any(|name| name == "tex"), "{names:?}");
    assert_eq!(
        pvfs.resolve_uuid("tex/a.png"),
        AssetUUID::new_v5("Tex/a.png"),
        "the asset lost its identity"
    );
    Ok(())
}

/// **A write reserved over a renamed file while the registry is unread never hands out an id the file does not hold.**
///
/// The registry is refused at the restricted trust, so the frozen identity
/// of `materials/b.kmat` is unknown: the id a slot gives there, which a
/// material's assignment or a prefab's own id is written as, must be the one
/// the file holds once the registry reads, or the slot refused.
#[test]
fn a_write_reserved_while_the_registry_is_unread_holds_the_files_identity() -> Result<()> {
    use khora_sdk::trust::{ProjectTrust, ProjectTrustState, Refusals, Trust};

    let project = TempProject::new("unread-slot-id")?;
    {
        let mut pvfs = project.open()?;
        pvfs.create_asset("materials/a.kmat", b"first")?;
        pvfs.rename_asset("materials/a.kmat", "materials/b.kmat")?;
    }
    let registry = AssetIdRegistry::unread(&project.0).file_path();
    let mut padded = std::fs::read(&registry)?;
    padded.resize((16 << 20) + 1, b' ');
    std::fs::write(&registry, padded)?;
    let refusals = Arc::new(Refusals::new(None, None));
    let trust = Arc::new(ProjectTrustState::new(ProjectTrust::Restricted));
    trust.set_for(&project.0, ProjectTrust::Restricted);
    let mut pvfs = ProjectVfs::over_service(
        project.0.clone(),
        Arc::new(Mutex::new(AssetService::empty())),
        Arc::new(AssetWatch::default()),
        ProjectReads::new(Arc::clone(&refusals), Arc::clone(&trust)),
    )?;

    let reserved = pvfs.reserve_write("materials/b.kmat");

    trust.set_for(&project.0, ProjectTrust::Trusted);
    refusals.release_strict_under(Trust::Restricted, &project.0);
    pvfs.reread_at_its_trust();
    let identity = pvfs.resolve_uuid("materials/b.kmat");
    assert_eq!(
        identity,
        AssetUUID::new_v5("materials/a.kmat"),
        "the premise: the registry, read, names the file by its rename"
    );
    if let Ok(slot) = reserved {
        assert_eq!(
            slot.id(),
            identity,
            "the slot hands out an id the file does not hold"
        );
    }
    Ok(())
}

/// **A reserved write over a file is refused while the registry is unread**: the id it holds is unknown.
///
/// A prefab writes that id into itself, a material is assigned by it; a
/// plain write over the file still lands.
#[test]
fn a_reserved_write_over_a_file_is_refused_while_the_registry_is_unread() -> Result<()> {
    let project = TempProject::new("unread-reserve-over")?;
    project.refuse_registry()?;
    let prefab = project.put("prefabs/crate.kprefab", b"first")?;
    let mut pvfs = project.open()?;

    refused_for_the_unread_registry("reserve", pvfs.reserve_write("prefabs/crate.kprefab"));
    assert_eq!(std::fs::read(&prefab)?, b"first");
    pvfs.write_asset(Path::new("prefabs/crate.kprefab"), b"second")?;
    assert_eq!(std::fs::read(&prefab)?, b"second");
    Ok(())
}

/// **A file renamed in another case only, where file names ignore case, keeps its identity.**
#[test]
fn a_file_renamed_in_another_case_only_keeps_its_identity() -> Result<()> {
    let project = TempProject::new("file-case-rename")?;
    if !ignores_case(&project) {
        return Ok(()); // The file system tells case apart: two files.
    }
    project.put("textures/A.png", b"first")?;
    let mut pvfs = project.open()?;

    pvfs.rename_asset("textures/A.png", "textures/a.png")?;

    let names: Vec<String> = std::fs::read_dir(project.0.join("assets").join("textures"))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["a.png"]);
    assert_eq!(
        pvfs.resolve_uuid("textures/a.png"),
        AssetUUID::new_v5("textures/A.png")
    );
    Ok(())
}

/// **A file trashed from a folder later renamed in case only gets its identity back once restored at its path.**
///
/// Where file names ignore case, `Tex/b.png` restored lands in `tex/`: it is
/// the path the trash kept its frozen identity at, and the restore must find it.
#[test]
fn a_file_trashed_from_a_folder_renamed_in_case_only_gets_its_identity_back() -> Result<()> {
    let project = TempProject::new("case-rename-trashed")?;
    if !ignores_case(&project) {
        return Ok(()); // The file system tells case apart: another folder.
    }
    let mut pvfs = project.open()?;
    pvfs.create_asset("Tex/a.png", b"first")?;
    pvfs.rename_asset("Tex/a.png", "Tex/b.png")?;
    let bin = project.0.join("bin");
    std::fs::create_dir_all(&bin)?;
    let binned = bin.join("b.png");
    pvfs.trash_with("Tex/b.png", |abs| Ok(std::fs::rename(abs, &binned)?))?;

    pvfs.rename_asset("Tex", "tex")?;
    std::fs::rename(&binned, project.0.join("assets").join("Tex").join("b.png"))?;
    pvfs.reindex()?;

    let moved = AssetUUID::new_v5("Tex/a.png");
    let listed = crate::hot_reload::collect_asset_entries(&pvfs);
    assert!(
        listed.iter().any(|entry| entry.uuid == moved),
        "the restored file lost the identity its references stored: {listed:?}"
    );
    Ok(())
}
