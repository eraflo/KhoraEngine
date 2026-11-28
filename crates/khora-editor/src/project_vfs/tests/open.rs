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

//! Opening a project: its decoders, its index, the engine's service it shares, what it reads of `project.json`.

use super::*;

#[test]
fn an_opened_project_registers_every_decoder() -> Result<()> {
    let root = std::env::temp_dir().join(format!(
        "khora-editor-project-decoders-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root)?;
    }
    let assets = root.join("assets");
    std::fs::create_dir_all(&assets)?;
    for file in PROBES {
        std::fs::write(assets.join(file), b"not a real asset")?;
    }

    let decodable = ProjectVfs::open(root.clone(), Arc::new(MetricsRegistry::new()))
        .and_then(|pvfs| decodable_types(&mut pvfs.assets()));
    let _ = std::fs::remove_dir_all(&root);

    assert_eq!(decodable?, DECODABLE);
    Ok(())
}

/// A renamed asset keeps the UUID its references stored.
///
/// An opened project indexes it under the identity `.khora/asset-registry.ron`
/// froze, not the one its new path would derive.
#[test]
fn an_opened_project_resolves_the_uuids_the_registry_froze() -> Result<()> {
    let root = std::env::temp_dir().join(format!(
        "khora-editor-project-registry-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root)?;
    }
    let scenes = root.join("assets").join("scenes");
    std::fs::create_dir_all(&scenes)?;
    std::fs::write(scenes.join("renamed.kscene"), b"scene bytes")?;

    let frozen = khora_sdk::khora_core::asset::AssetUUID::new_v5("scenes/original.kscene");
    let mut registry =
        khora_sdk::AssetIdRegistry::load_or_unread(&khora_sdk::trust::FileGuard::new(
            &root,
            khora_sdk::trust::Trust::Project,
            khora_sdk::trust::Refusals::detached(),
        ));
    registry.freeze("scenes/renamed.kscene", frozen);
    registry.save()?;

    let resolved = ProjectVfs::open(root.clone(), Arc::new(MetricsRegistry::new())).map(|pvfs| {
        let by_frozen = pvfs.assets().load_raw(&frozen).ok();
        let by_path = pvfs
            .assets()
            .index()
            .get_metadata(&khora_sdk::khora_core::asset::AssetUUID::new_v5(
                "scenes/renamed.kscene",
            ))
            .is_some();
        (by_frozen, by_path)
    });
    let _ = std::fs::remove_dir_all(&root);
    let (by_frozen, by_path) = resolved?;

    assert_eq!(by_frozen.as_deref(), Some(&b"scene bytes"[..]));
    assert!(!by_path, "indexed under its path-derived UUID");
    Ok(())
}

/// **A project opened freezes the derived UUIDs of the index it opened with.**
///
/// With `textures/a.png` renamed to `textures/b.png` and a file made again
/// at `textures/a.png` while the editor was closed: the index the project
/// opens over gives it a derived UUID, which the opened project freezes,
/// so it stays that file's once `textures/b.png` is gone.
#[test]
fn a_project_opened_freezes_the_derived_uuids_of_its_index() -> Result<()> {
    let project = TempProject::new("open-freezes-derived")?;
    let textures = project.0.join("assets").join("textures");
    std::fs::create_dir_all(&textures)?;
    std::fs::write(textures.join("a.png"), b"second")?;
    std::fs::write(textures.join("b.png"), b"first")?;
    let mut registry = AssetIdRegistry::load_or_unread(&khora_sdk::trust::FileGuard::new(
        &project.0,
        khora_sdk::trust::Trust::Project,
        khora_sdk::trust::Refusals::detached(),
    ));
    registry.rename("textures/a.png", "textures/b.png");
    registry.save()?;
    let derived = registry.resolve("textures/a.png");
    let opened = AssetService::open_loose_files(
        &project.0,
        &AssetIdRegistry::load_or_unread(&khora_sdk::trust::FileGuard::new(
            &project.0,
            khora_sdk::trust::Trust::Project,
            khora_sdk::trust::Refusals::detached(),
        )),
        Arc::new(MetricsRegistry::new()),
    )?;
    let shared = Arc::new(Mutex::new(opened));

    let _pvfs = ProjectVfs::over_service(
        project.0.clone(),
        Arc::clone(&shared),
        Arc::new(AssetWatch::default()),
        ProjectReads::default(),
    )?;

    let saved = AssetIdRegistry::load_or_unread(&khora_sdk::trust::FileGuard::new(
        &project.0,
        khora_sdk::trust::Trust::Project,
        khora_sdk::trust::Refusals::detached(),
    ));
    assert_eq!(saved.resolve("textures/a.png"), derived);
    assert_eq!(
        saved.len(),
        2,
        "the derived UUID of `textures/a.png` is not frozen at the open"
    );
    Ok(())
}

/// The engine hears of each copy made stale by another project's service moving in.
///
/// A path both projects share is one uuid, so what the previous service
/// served is stale once the next moves in.
#[test]
fn sharing_another_projects_assets_announces_what_the_previous_one_served() -> Result<()> {
    const REL: &str = "meshes/a.obj";
    let first = TempProject::new("share-first")?;
    let second = TempProject::new("share-second")?;
    for (project, obj) in [
        (&first, "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n"),
        (&second, "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nf 1 2 3 4\n"),
    ] {
        let meshes = project.0.join("assets").join("meshes");
        std::fs::create_dir_all(&meshes)?;
        std::fs::write(meshes.join("a.obj"), obj)?;
    }
    let mesh = AssetUUID::new_v5(REL);
    let shared = Arc::new(Mutex::new(AssetService::empty()));

    let mut first_vfs = first.open()?;
    first_vfs.share_assets(&shared);
    first_vfs.assets().load::<Mesh>(&mesh)?;

    let mut second_vfs = second.open()?;
    second_vfs.share_assets(&shared);

    let announced: Vec<AssetUUID> = shared
        .lock_recovered()
        .changes()
        .drain()
        .into_iter()
        .map(|change| change.0)
        .collect();
    assert!(
        announced.contains(&mesh),
        "the mesh the first project served is announced: {announced:?}"
    );
    Ok(())
}

/// The editor's prefab source reads a glTF scene sub-asset as the scene's
/// record: its root, then the node, showing the file's sub-assets.
#[test]
fn a_gltf_scene_is_a_prefab_record_of_the_project() -> Result<()> {
    use khora_sdk::khora_data::ecs::{MeshRef, Name};

    let root = std::env::temp_dir().join(format!(
        "khora-editor-project-gltf-prefab-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root)?;
    }
    let models = root.join("assets").join("models");
    std::fs::create_dir_all(&models)?;
    std::fs::write(models.join("ship.gltf"), ONE_NODE_GLTF)?;

    let file = AssetUUID::new_v5("models/ship.gltf");
    let scene = AssetUUID::sub_asset(file, "scene");
    let record = ProjectVfs::open(root.clone(), Arc::new(MetricsRegistry::new()))
        .map(|mut pvfs| pvfs.prefab_record(scene));
    let _ = std::fs::remove_dir_all(&root);
    let record = record?.map_err(|e| anyhow::anyhow!("the scene reads: {e}"))?;

    let mut world = khora_sdk::khora_data::ecs::World::new();
    khora_sdk::khora_data::scene::apply(
        &mut world,
        &record,
        khora_sdk::khora_data::scene::Identity::Keep,
    )
    .map_err(|e| anyhow::anyhow!("the record applies: {e}"))?;
    assert_eq!(record.entities.len(), 2, "the root and Hull");
    let hull = world
        .iter_entities()
        .find(|e| world.get::<Name>(*e).is_some_and(|n| n.as_str() == "Hull"))
        .expect("Hull is in the record");
    assert_eq!(
        world.get::<MeshRef>(hull).cloned(),
        Some(MeshRef::Asset(AssetUUID::sub_asset(file, "mesh0/prim0")))
    );
    Ok(())
}

/// **A project opened over the engine's service is not walked.**
///
/// Its index comes through the engine's `IndexHub`. The identity
/// registry's file is made and its assets are watched, as `open_watched` does.
#[test]
fn a_project_opened_over_the_engines_service_is_not_walked() -> Result<()> {
    let project = TempProject::new("over-service-not-walked")?;
    let textures = project.0.join("assets").join("textures");
    std::fs::create_dir_all(&textures)?;
    std::fs::write(textures.join("crate.png"), b"PNG")?;
    let shared = Arc::new(Mutex::new(AssetService::empty()));
    let watch = Arc::new(AssetWatch::default());

    let pvfs = ProjectVfs::over_service(
        project.0.clone(),
        Arc::clone(&shared),
        watch.clone(),
        ProjectReads::default(),
    )?;

    assert!(
        Arc::ptr_eq(&pvfs.asset_service, &shared),
        "the project serves the engine's service, not one of its own"
    );
    assert_eq!(
        pvfs.assets().index().asset_count(),
        0,
        "the project's files were walked on the frame"
    );
    assert!(
        pvfs.assets()
            .index()
            .get_metadata(&AssetUUID::new_v5("textures/crate.png"))
            .is_none(),
        "the file is indexed before any `Open` landed"
    );
    assert!(
        project
            .0
            .join(".khora")
            .join("asset-registry.ron")
            .is_file(),
        "the identity registry's file is not made"
    );
    let assets = std::fs::canonicalize(project.0.join("assets"))?;
    assert!(
        watch
            .roots()
            .iter()
            .filter_map(|root| std::fs::canonicalize(root).ok())
            .any(|root| root == assets),
        "the project's assets are not watched: {:?}",
        watch.roots()
    );
    Ok(())
}

/// **A project opened over the engine's service gets its `assets/`.**
///
/// A project folder with no `assets/` yet opens with one, empty: the
/// writer and the watch have a folder to work in.
#[test]
fn a_project_opened_over_the_engines_service_gets_its_assets_folder() -> Result<()> {
    let project = TempProject::new("over-service-assets-folder")?;
    std::fs::remove_dir_all(project.0.join("assets"))?;
    let shared = Arc::new(Mutex::new(AssetService::empty()));

    let pvfs = ProjectVfs::over_service(
        project.0.clone(),
        shared,
        Arc::new(AssetWatch::default()),
        ProjectReads::default(),
    )?;

    assert!(
        project.0.join("assets").is_dir(),
        "the project's `assets/` is not created"
    );
    assert_eq!(pvfs.assets_root, project.0.join("assets"));
    Ok(())
}

/// **The project's physics step is read from its `project.json`.**
///
/// A project with no section, no `project.json`, or a section that does
/// not read (a zero step) runs at the default step.
#[test]
fn the_physics_settings_are_the_physics_section_of_project_json() -> Result<()> {
    let a_128th = std::time::Duration::from_nanos(7_812_500);
    let project = TempProject::new("physics-settings")?;
    let read = |json: Option<&str>| -> Result<PhysicsSettings> {
        let path = project.0.join("project.json");
        match json {
            Some(json) => std::fs::write(&path, json)?,
            None => {
                let _ = std::fs::remove_file(&path);
            }
        }
        Ok(project.open()?.physics_settings())
    };

    assert_eq!(
        read(Some(r#"{ "name": "p", "physics": { "step": 0.0078125 } }"#))?.step,
        a_128th,
        "the section's step is the project's"
    );
    assert_eq!(
        read(Some(r#"{ "name": "p" }"#))?,
        PhysicsSettings::default()
    );
    assert_eq!(read(None)?, PhysicsSettings::default());
    assert_eq!(
        read(Some(r#"{ "name": "p", "physics": { "step": 0 } }"#))?,
        PhysicsSettings::default(),
        "a step that does not read leaves the default"
    );
    Ok(())
}
