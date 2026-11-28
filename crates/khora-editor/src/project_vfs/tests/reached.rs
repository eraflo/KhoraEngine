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

//! Which files are the project's: under its assets as named, as spelled, through a link.

use super::*;

/// **A file inside the project's assets is named by its path under them, however the project is named.**
///
/// Through a `..`, and in another case where file names ignore case; the
/// assets folder itself and a file beside it are not inside.
#[test]
fn rel_inside_assets_names_a_file_however_the_project_is_named() -> Result<()> {
    let project = TempProject::new("inside-assets")?;
    let pvfs = project.open()?;
    let assets = project.0.join("assets");
    std::fs::create_dir_all(project.0.join("x"))?;

    assert_eq!(
        pvfs.rel_inside_assets(&assets.join("scenes").join("a.kscene")),
        Some("scenes/a.kscene".to_owned())
    );
    assert_eq!(
        pvfs.rel_inside_assets(&project.0.join("x").join("..").join("assets").join("b.kmat")),
        Some("b.kmat".to_owned())
    );
    let other_case = PathBuf::from(assets.to_string_lossy().to_uppercase());
    if other_case.exists() && other_case != assets {
        assert_eq!(
            pvfs.rel_inside_assets(&other_case.join("c.kmat")),
            Some("c.kmat".to_owned())
        );
    }
    assert_eq!(pvfs.rel_inside_assets(&assets), None);
    assert_eq!(
        pvfs.rel_inside_assets(&project.0.join("project.json")),
        None
    );
    Ok(())
}

/// **A file picked through a folder named in another case is named as the folder is spelled on disk.**
///
/// The scene `scenes/level.kscene` picked as `SCENES/LEVEL.kscene`, where
/// file names ignore case, is the indexed scene: opened by the path returned,
/// it is found.
#[test]
fn rel_inside_assets_names_a_file_under_a_folder_in_another_case_as_it_is_spelled() -> Result<()> {
    let project = TempProject::new("inside-assets-folder-case")?;
    if !ignores_case(&project) {
        return Ok(()); // The file system tells case apart: another file.
    }
    project.put("scenes/level.kscene", b"scene bytes")?;
    let mut pvfs = project.open()?;
    pvfs.rebuild_index()?;
    assert!(
        pvfs.indexed_and_present("scenes/level.kscene"),
        "the premise: the scene is indexed"
    );

    let picked = project.0.join("assets").join("SCENES").join("LEVEL.kscene");
    let rel = pvfs.rel_inside_assets(&picked);

    assert_eq!(rel.as_deref(), Some("scenes/level.kscene"));
    assert!(
        rel.is_some_and(|rel| pvfs.indexed_and_present(&rel)),
        "the scene picked is not found by the path returned"
    );
    Ok(())
}

/// **A file through a link in the assets that leads out of them is not the project's.**
///
/// The trust boundary refuses to read through such a link, and the index
/// never walks it: a scene saved there would never open again. It is
/// outside, and a write there is refused, nothing written.
#[test]
fn a_file_through_a_link_leaving_the_assets_is_not_the_projects() -> Result<()> {
    let project = TempProject::new("link-leaves-assets")?;
    let elsewhere = project.0.join("elsewhere");
    std::fs::create_dir_all(&elsewhere)?;
    let linked = project.0.join("assets").join("linked");
    if !link_dir(&elsewhere, &linked) {
        return Ok(()); // The system makes no link.
    }
    let mut pvfs = project.open()?;

    assert_eq!(
        pvfs.rel_inside_assets(&linked.join("a.kscene")),
        None,
        "a file through the link is taken as the project's"
    );
    let written = pvfs.write_asset(Path::new("linked/a.kscene"), b"scene bytes");
    assert!(
        !elsewhere.join("a.kscene").exists(),
        "the write left the assets through the link: {written:?}"
    );
    assert!(written.is_err(), "the write through the link is accepted");
    Ok(())
}

/// **A new name picked that is no name a project keeps is refused, naming it, never trimmed.**
///
/// A name ending with dots or spaces is dropped by Windows and kept elsewhere,
/// a device's name is no file on Windows: neither is a path the project
/// keeps on every system, so nothing is read or written there.
#[test]
fn in_project_refuses_a_new_name_no_system_keeps_as_typed_naming_it() -> Result<()> {
    let project = TempProject::new("inside-assets-refused")?;
    let pvfs = project.open()?;
    let scenes = project.0.join("assets").join("scenes");

    for name in [
        "new.kscene.",
        "new.kscene  ",
        "...",
        "NUL",
        "aux.kscene",
        "a?.kscene",
    ] {
        match pvfs.in_project(&scenes.join(name)) {
            InProject::Refused(why) => assert!(why.ends_with(&format!("'{name}'")), "{why}"),
            other => panic!("'{name}' is not refused: {other:?}"),
        }
    }
    assert_eq!(
        pvfs.in_project(&scenes.join("new.kscene")),
        InProject::Inside("scenes/new.kscene".to_owned())
    );
    assert_eq!(
        pvfs.in_project(&project.0.join("project.json")),
        InProject::Outside
    );
    Ok(())
}

/// **A path named under the assets that a link leads out of is refused, never written as a file outside.**
#[test]
fn in_project_refuses_a_path_through_a_link_leaving_the_assets() -> Result<()> {
    let project = TempProject::new("inside-assets-link-refused")?;
    let elsewhere = project.0.join("elsewhere");
    std::fs::create_dir_all(&elsewhere)?;
    let linked = project.0.join("assets").join("linked");
    if !link_dir(&elsewhere, &linked) {
        return Ok(()); // The system makes no link.
    }
    let pvfs = project.open()?;

    assert!(matches!(
        pvfs.in_project(&linked.join("a.kscene")),
        InProject::Refused(_)
    ));
    Ok(())
}

/// **A file picked in a folder that exists is never named by another folder's name.**
///
/// A folder `v1.` (made on Windows through a resolved path, any name
/// elsewhere) beside a folder `v1`: the path returned for the scene picked in
/// `v1.` must name that scene, or none.
#[test]
fn rel_inside_assets_never_names_a_file_in_an_existing_folder_by_another_folders_name() -> Result<()>
{
    let project = TempProject::new("inside-assets-dotted-folder")?;
    project.put("v1/level.kscene", b"plain")?;
    let pvfs = project.open()?;
    // Resolved, so that Windows keeps the trailing dot of the name.
    let dotted = std::fs::canonicalize(project.0.join("assets"))?.join("v1.");
    std::fs::create_dir(&dotted)?;
    let picked = dotted.join("level.kscene");
    std::fs::write(&picked, b"dotted")?;

    let rel = pvfs.rel_inside_assets(&picked);

    if let Some(rel) = &rel {
        assert_eq!(
            std::fs::read(pvfs.abs_of(rel))?,
            b"dotted",
            "'{rel}' names another file than the one picked"
        );
    }
    Ok(())
}
