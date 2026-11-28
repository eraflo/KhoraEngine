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

//! A write reserved for a file, and refused once the project changed it.

use super::*;

/// **A write reserved over a file named in another case lands on that file, under its identity.**
#[test]
fn reserve_write_names_a_file_under_another_case_as_it_is_spelled() -> Result<()> {
    let project = TempProject::new("reserve-other-case")?;
    project.put("textures/a.png", b"first")?;
    let mut pvfs = project.open()?;
    if !project
        .0
        .join("assets")
        .join("TEXTURES")
        .join("A.png")
        .exists()
    {
        return Ok(()); // The file system tells case apart: another file.
    }

    let slot = pvfs.reserve_write("TEXTURES/A.png")?;

    assert_eq!(slot.rel(), "textures/a.png");
    assert_eq!(slot.id(), AssetUUID::new_v5("textures/a.png"));
    assert_eq!(
        pvfs.write_reserved(slot, b"second")?,
        AssetUUID::new_v5("textures/a.png")
    );
    assert_eq!(
        std::fs::read(project.0.join("assets/textures/a.png"))?,
        b"second"
    );
    Ok(())
}

/// **A write reserved is refused once the project changed so the file would hold another id.**
///
/// A pull renamed `textures/c.png` to `textures/z.png` after the write to
/// `textures/c.png` was reserved: `c`'s default is `z`'s now, so the file
/// would hold another id than the one reserved, and nothing is written.
#[test]
fn write_reserved_refuses_a_slot_the_project_changed_since() -> Result<()> {
    let project = TempProject::new("reserve-then-pull")?;
    let mut pvfs = project.open()?;
    let slot = pvfs.reserve_write("textures/c.png")?;
    assert_eq!(slot.id(), AssetUUID::new_v5("textures/c.png"));

    let mut pulled = saved_registry(&project.0);
    pulled.rename("textures/c.png", "textures/z.png");
    pulled.save()?;

    assert!(pvfs.write_reserved(slot, b"mine").is_err());
    assert!(
        !project.0.join("assets/textures/c.png").exists(),
        "the file was written"
    );
    let again = pvfs.reserve_write("textures/c.png")?;
    assert_ne!(again.id(), AssetUUID::new_v5("textures/c.png"));
    assert_eq!(pvfs.write_reserved(again.clone(), b"mine")?, again.id());
    Ok(())
}

/// **A write reserved outside `assets/` is refused.**
#[test]
fn reserve_write_refuses_a_path_outside_the_assets() -> Result<()> {
    let project = TempProject::new("reserve-outside")?;
    let mut pvfs = project.open()?;
    assert!(pvfs.reserve_write("../project.json").is_err());
    assert!(pvfs.reserve_write("").is_err());
    Ok(())
}
