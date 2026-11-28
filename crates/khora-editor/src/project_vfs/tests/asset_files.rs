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

//! Making, renaming, moving and trashing a project's files, and the names refused.

use super::*;

#[test]
fn create_asset_writes_the_file_and_its_folders_and_indexes_it() -> Result<()> {
    let project = TempProject::new("writes")?;
    let mut pvfs = project.open()?;

    pvfs.create_asset("scripts/ai/Player.erg", b"behavior Player { }")?;

    let written = project
        .0
        .join("assets")
        .join("scripts")
        .join("ai")
        .join("Player.erg");
    assert_eq!(std::fs::read(&written)?, b"behavior Player { }");
    let listed = crate::hot_reload::collect_asset_entries(&pvfs);
    assert!(
        listed
            .iter()
            .any(|entry| entry.source_path == "scripts/ai/Player.erg"),
        "the index does not list the new file: {listed:?}"
    );
    Ok(())
}

#[test]
fn create_asset_refuses_an_existing_file_and_leaves_its_bytes() -> Result<()> {
    let project = TempProject::new("existing")?;
    let scripts = project.0.join("assets").join("scripts");
    std::fs::create_dir_all(&scripts)?;
    std::fs::write(scripts.join("Player.erg"), b"what the author wrote")?;
    let mut pvfs = project.open()?;

    let created = pvfs.create_asset("scripts/Player.erg", b"a template");

    assert!(created.is_err(), "an existing file is overwritten");
    assert_eq!(
        std::fs::read(scripts.join("Player.erg"))?,
        b"what the author wrote"
    );
    Ok(())
}

#[test]
fn create_asset_refuses_a_path_that_is_empty_or_leaves_or_bends_assets() -> Result<()> {
    let project = TempProject::new("refused")?;
    let outside = std::env::temp_dir().join(format!(
        "khora-editor-create-asset-outside-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&outside);
    let absolute = outside.join("absolute.erg");
    let absolute_fwd = absolute.to_string_lossy().replace('\\', "/");
    let mut pvfs = project.open()?;

    let refused = [
        "",
        "../escape.erg",
        "a/../b.erg",
        "./here.erg",
        "a//b.erg",
        "a/",
        "/rooted.erg",
        absolute_fwd.as_str(),
    ]
    .map(|rel| (rel.to_owned(), pvfs.create_asset(rel, b"bytes").is_err()));
    let absolute_written = absolute.exists();
    let _ = std::fs::remove_dir_all(&outside);

    for (rel, was_refused) in &refused {
        assert!(was_refused, "`{rel}` was accepted");
    }
    assert!(
        !absolute_written,
        "{} was written by an absolute path",
        absolute.display()
    );
    let assets = project.0.join("assets");
    for written in [
        project.0.join("escape.erg"),
        assets.join("b.erg"),
        assets.join("a"),
        assets.join("here.erg"),
        assets.join("rooted.erg"),
    ] {
        assert!(
            !written.exists(),
            "{} was written by a refused path",
            written.display()
        );
    }
    Ok(())
}

/// **A rename to a path that leaves or bends the assets is refused, nothing moved.**
///
/// A name typed in the asset browser holding `..` would move the folder out
/// of the project; one holding a backslash would make folders the registry
/// names otherwise, losing the identity of what moved.
#[test]
fn a_rename_to_a_path_that_leaves_or_bends_the_assets_is_refused() -> Result<()> {
    let project = TempProject::new("rename-leaves-assets")?;
    let mut pvfs = project.open()?;
    pvfs.create_asset("textures/a.png", b"first")?;
    let kept = project.0.join("assets").join("textures").join("a.png");

    let climbed = pvfs.rename_asset("textures", "../escaped");
    assert!(
        !project.0.join("escaped").exists(),
        "the folder left the assets: {climbed:?}"
    );
    assert!(climbed.is_err(), "a rename out of the assets is accepted");

    let bent = pvfs.rename_asset("textures/a.png", "textures/hd\\b.png");
    assert!(bent.is_err(), "a rename through a backslash is accepted");
    assert!(kept.exists(), "the file moved");
    Ok(())
}

/// **A Save As refused under the assets writes nothing, inside or outside the project.**
#[test]
fn a_scene_saved_as_a_refused_path_under_the_assets_writes_nothing() -> Result<()> {
    let project = TempProject::new("save-as-refused")?;
    let vfs = Arc::new(Mutex::new(project.open()?));
    let world = khora_sdk::GameWorld::new();
    let scenes = project.0.join("assets").join("scenes");
    std::fs::create_dir_all(&scenes)?;

    let picked = scenes.join("level.kscene.");
    let saved = crate::commands::save_scene_dispatch(Some(&vfs), &world, &picked.to_string_lossy());

    assert!(!saved, "a refused path is said saved");
    assert_eq!(
        std::fs::read_dir(&scenes)?.count(),
        0,
        "something was written"
    );
    Ok(())
}

/// **A folder made, a file made or a rename to a new name is held to the portable name rules on every system.**
#[test]
fn a_new_name_no_system_keeps_is_refused_for_every_new_entry() -> Result<()> {
    let project = TempProject::new("new-names-refused")?;
    let mut pvfs = project.open()?;
    pvfs.create_asset("textures/a.png", b"first")?;

    assert!(pvfs.create_asset("textures/b.png.", b"x").is_err());
    assert!(pvfs.create_asset("textures/con.png", b"x").is_err());
    assert!(pvfs.create_asset("new?/b.png", b"x").is_err());
    assert!(pvfs.create_folder("new ").is_err());
    assert!(pvfs
        .rename_asset("textures/a.png", "textures/a|b.png")
        .is_err());
    assert!(pvfs.reserve_write("prefabs/lpt1.kprefab").is_err());
    assert!(pvfs
        .write_asset(Path::new("scenes/prn.kscene"), b"x")
        .is_err());

    assert_eq!(
        std::fs::read_dir(project.0.join("assets"))?.count(),
        1,
        "something was made beside textures/"
    );
    assert_eq!(
        std::fs::read_dir(project.0.join("assets").join("textures"))?.count(),
        1
    );
    Ok(())
}

/// **A file named on another system, outside the portable rules, can still be renamed to a portable name, moved or trashed.**
#[cfg(unix)]
#[test]
fn a_file_with_a_name_only_its_system_keeps_is_renamed_moved_and_trashed() -> Result<()> {
    let project = TempProject::new("non-portable-existing")?;
    project.put("textures/a?.png", b"first")?;
    project.put("textures/b:c.png", b"second")?;
    project.put("textures/d<e>.png", b"third")?;
    std::fs::create_dir_all(project.0.join("assets").join("moved"))?;
    let mut pvfs = project.open()?;

    pvfs.rename_asset("textures/a?.png", "textures/a.png")?;
    pvfs.move_asset("textures/b:c.png", "moved")?;
    let bin = project.0.join("bin");
    std::fs::create_dir_all(&bin)?;
    pvfs.trash_with("textures/d<e>.png", |abs| {
        Ok(std::fs::rename(abs, bin.join("d.png"))?)
    })?;

    let assets = project.0.join("assets");
    assert!(assets.join("textures").join("a.png").is_file());
    assert!(assets.join("moved").join("b:c.png").is_file());
    assert!(bin.join("d.png").is_file());
    assert_eq!(
        pvfs.resolve_uuid("textures/a.png"),
        AssetUUID::new_v5("textures/a?.png")
    );
    Ok(())
}

/// **A file named like a device with an extension, which this Windows made and reaches by its name, is renamed, moved and trashed.**
///
/// Windows 11 makes `con.png` a file of its folder, and the browser lists it:
/// renaming it is how its author makes it portable. Skipped where Windows
/// reads the name as the device and makes no such file.
#[cfg(windows)]
#[test]
fn a_device_named_file_this_windows_reaches_is_renamed_moved_and_trashed() -> Result<()> {
    let project = TempProject::new("device-named-existing")?;
    let made = project.put("textures/con.png", b"first")?;
    let listed = std::fs::read_dir(made.parent().expect("a folder"))?
        .filter_map(|entry| entry.ok())
        .any(|entry| entry.file_name() == "con.png");
    if !listed || std::fs::read(&made)? != b"first" {
        return Ok(()); // This Windows reads `con.png` as the console.
    }
    project.put("textures/aux.tar.gz", b"second")?;
    project.put("textures/nul.txt", b"third")?;
    std::fs::create_dir_all(project.0.join("assets").join("moved"))?;
    let mut pvfs = project.open()?;

    let renamed = pvfs.rename_asset("textures/con.png", "textures/console.png");
    let moved = pvfs.move_asset("textures/aux.tar.gz", "moved");
    let bin = project.0.join("bin");
    std::fs::create_dir_all(&bin)?;
    let trashed = pvfs.trash_with("textures/nul.txt", |abs| {
        Ok(std::fs::rename(abs, bin.join("nul.txt"))?)
    });

    let failed: Vec<String> = [renamed, moved, trashed]
        .into_iter()
        .filter_map(|outcome| outcome.err().map(|error| format!("{error:#}")))
        .collect();
    assert!(failed.is_empty(), "refused though reached: {failed:#?}");
    Ok(())
}

/// **An entry a case-only rename cut short left aside is put back at the project's open**, and one whose name is taken is left.
#[test]
fn reclaim_asides_puts_back_what_a_case_only_rename_left_and_keeps_what_it_cannot() -> Result<()> {
    let project = TempProject::new("reclaim-asides")?;
    let assets = project.0.join("assets");
    let aside = assets.join(format!("Tex.{}.0.aside.tmp", std::process::id()));
    std::fs::create_dir_all(&aside)?;
    std::fs::write(aside.join("a.png"), b"first")?;
    project.put("scenes/level.kscene", b"kept")?;
    let taken = assets.join("scenes").join("level.kscene.1.2.aside.tmp");
    std::fs::write(&taken, b"aside")?;
    let mut pvfs = project.open()?;
    assert!(
        !pvfs.indexed_and_present("Tex/a.png"),
        "the premise: what is aside is not indexed"
    );

    let reclaimed = pvfs.reclaim_asides();

    assert_eq!(reclaimed.len(), 2, "{reclaimed:?}");
    assert!(
        pvfs.indexed_and_present("Tex/a.png"),
        "the folder is not back"
    );
    assert_eq!(
        std::fs::read(&taken)?,
        b"aside",
        "the entry left is not kept"
    );
    assert_eq!(
        std::fs::read(assets.join("scenes").join("level.kscene"))?,
        b"kept"
    );
    assert!(!pvfs.list_dirs().iter().any(|dir| dir.ends_with(".tmp")));
    Ok(())
}

/// **A file made or moved through a link leaving the assets is refused, nothing made or moved.**
#[test]
fn a_file_made_or_moved_through_a_link_leaving_the_assets_is_refused() -> Result<()> {
    let project = TempProject::new("link-made-moved")?;
    let elsewhere = project.0.join("elsewhere");
    std::fs::create_dir_all(&elsewhere)?;
    if !link_dir(&elsewhere, &project.0.join("assets").join("linked")) {
        return Ok(()); // The system makes no link.
    }
    let kept = project.put("textures/a.png", b"first")?;
    let mut pvfs = project.open()?;

    assert!(pvfs.create_asset("linked/b.erg", b"x").is_err());
    assert!(pvfs.rename_asset("textures/a.png", "linked/a.png").is_err());
    assert!(pvfs.create_folder("linked/sub").is_err());

    assert!(kept.exists(), "the file moved");
    assert_eq!(
        std::fs::read_dir(&elsewhere)?.count(),
        0,
        "something was made through the link"
    );
    Ok(())
}

/// **A file made under a folder named in another case is filed as the folder is spelled on disk.**
#[test]
fn a_file_made_under_a_folder_in_another_case_is_filed_as_it_is_spelled() -> Result<()> {
    let project = TempProject::new("made-folder-case")?;
    if !ignores_case(&project) {
        return Ok(()); // The file system tells case apart: another folder.
    }
    project.put("Scripts/a.erg", b"behavior A { }")?;
    let mut pvfs = project.open()?;

    pvfs.create_asset("scripts/b.erg", b"behavior B { }")?;

    assert!(pvfs.indexed_and_present("Scripts/b.erg"));
    Ok(())
}

/// **A new name picked is never resolved onto a file that is there.**
///
/// Where `level.kscene.` names a file of its own (every system but Windows),
/// a save at that new name must not write over `level.kscene`.
#[test]
fn a_new_name_ending_with_a_dot_is_never_resolved_onto_another_file() -> Result<()> {
    let project = TempProject::new("inside-assets-dotted-new")?;
    project.put("scenes/level.kscene", b"kept")?;
    let pvfs = project.open()?;
    let picked = project
        .0
        .join("assets")
        .join("scenes")
        .join("level.kscene.");
    if picked.exists() {
        return Ok(()); // Windows: the name is the file's own.
    }

    let rel = pvfs.rel_inside_assets(&picked);

    assert_ne!(
        rel.as_deref(),
        Some("scenes/level.kscene"),
        "a new name resolves onto the file beside it"
    );
    Ok(())
}

/// **A file or a folder named as a Windows device is never said made when nothing is.**
///
/// `NUL` names the null device in every Windows folder: what is written there is dropped.
#[test]
fn a_name_windows_keeps_for_a_device_is_never_said_made() -> Result<()> {
    let project = TempProject::new("device-names")?;
    let mut pvfs = project.open()?;
    let names = |folder: &Path| -> Result<Vec<String>> {
        Ok(std::fs::read_dir(folder)?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().to_lowercase())
            .collect())
    };

    if pvfs.create_asset("scripts/NUL", b"behavior A { }").is_ok() {
        let made = names(&project.0.join("assets").join("scripts"))?;
        assert!(
            made.iter().any(|name| name == "nul"),
            "said made, yet {made:?}"
        );
    }
    if pvfs.create_folder("nul").is_ok() {
        let made = names(&project.0.join("assets"))?;
        assert!(
            made.iter().any(|name| name == "nul"),
            "said made, yet {made:?}"
        );
    }
    Ok(())
}
