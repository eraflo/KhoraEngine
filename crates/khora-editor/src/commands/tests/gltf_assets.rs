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

//! The drop requests carrying an asset's identity, processed against a
//! project on disk: a glTF scene spawned as a linked instance of its nodes, a
//! part of a file spawned or assigned by its own identity.

use khora_sdk::khora_core::asset::{AssetUUID, Material, StandardMaterial};
use khora_sdk::khora_data::ecs::PrefabInstance;

use super::super::*;
use crate::gltf_fixture::{GltfProject, HULL_COLOR, MAST_COLOR, SHIP_GLTF, TRIANGLE_OBJ};

const SHIP: &str = "models/ship.gltf";
const CRATE: &str = "models/crate.obj";
const WOOD: &str = "textures/wood.png";
const STEEL: &str = "materials/steel.kmat";

fn ship_project(name: &str) -> GltfProject {
    GltfProject::with(name, &[(SHIP, SHIP_GLTF)])
}

/// The entity of `world` named `name`.
fn named(world: &GameWorld, name: &str) -> EntityId {
    world
        .iter_entities()
        .find(|e| {
            world
                .get_component::<Name>(*e)
                .is_some_and(|n| n.as_str() == name)
        })
        .unwrap_or_else(|| {
            let names: Vec<String> = world
                .iter_entities()
                .filter_map(|e| {
                    world
                        .get_component::<Name>(e)
                        .map(|n| n.as_str().to_owned())
                })
                .collect();
            panic!("no entity named `{name}` among {names:?}")
        })
}

/// The material the project decodes for `uuid`.
fn decoded_material(project: &GltfProject, uuid: AssetUUID) -> StandardMaterial {
    let mut pvfs = project.vfs.lock().expect("the project");
    let handle = pvfs
        .asset_service
        .load::<Box<dyn Material>>(&uuid)
        .unwrap_or_else(|e| panic!("{uuid:?} decodes as a material: {e:#}"));
    let material: &dyn Material = &**handle;
    material
        .as_any()
        .downcast_ref::<StandardMaterial>()
        .expect("a glTF material is a StandardMaterial")
        .clone()
}

fn assert_color(material: &StandardMaterial, expected: [f32; 4], what: &str) {
    let c = material.base_color;
    let got = [c.r, c.g, c.b, c.a];
    for (g, e) in got.iter().zip(expected.iter()) {
        assert!(
            (g - e).abs() < 1e-5,
            "{what}: base colour {got:?}, the file says {expected:?}"
        );
    }
}

/// What an entity's material reference holds, in words.
fn worn(material: Option<&MaterialRef>) -> String {
    match material {
        None => "no material".to_owned(),
        Some(MaterialRef::Asset(uuid)) => format!("the material asset {uuid:?}"),
        Some(MaterialRef::Inline { uuid, .. }) => format!("the inline material {uuid:?}"),
    }
}

/// An entity standing in the scene, a drop target.
fn target(world: &mut GameWorld) -> EntityId {
    world.spawn((Transform::identity(), Name::new("Target")))
}

// ─── Spawning a glTF scene ──────────────────────────────────────────────────

/// **Spawning a glTF file's scene places each of its nodes, each in its own
/// material**: one linked instance whose root is named after the scene, and
/// under it `Hull` and `Mast`, each with its primitive's mesh and its own
/// material — and those materials decode to the colours the file gives them.
#[test]
fn spawning_a_gltf_scene_places_its_nodes_each_in_its_own_material() {
    let project = ship_project("spawn-scene");
    let scene = project.sub(SHIP, "scene");
    let mut world = GameWorld::new();
    let state = Arc::new(Mutex::new(EditorState::default()));
    state.lock().expect("the editor state").pending_prefab_spawn = Some((scene, None));

    process_pending_prefab_spawn(Some(&project.vfs), &mut world, &state);

    let roots: Vec<(EntityId, AssetUUID)> = world
        .iter_entities()
        .filter_map(|e| {
            world
                .get_component::<PrefabInstance>(e)
                .map(|link| (e, link.prefab))
        })
        .collect();
    assert_eq!(roots.len(), 1, "one instance: {roots:?}");
    let (root, prefab) = roots[0];
    assert_eq!(prefab, scene, "the instance links to the file's scene");
    assert_eq!(
        world
            .get_component::<Name>(root)
            .map(|n| n.as_str().to_owned()),
        Some("Ship".to_owned()),
        "the root is named after the scene"
    );
    assert_eq!(world.iter_entities().count(), 3, "the root, Hull and Mast");

    for (node, mesh, material, color) in [
        ("Hull", "mesh0/prim0", "material0", HULL_COLOR),
        ("Mast", "mesh1/prim0", "material1", MAST_COLOR),
    ] {
        let entity = named(&world, node);
        assert_eq!(
            world.get_component::<Parent>(entity).map(|p| p.0),
            Some(root),
            "{node} hangs under the root"
        );
        assert_eq!(
            world.get_component::<MeshRef>(entity).cloned(),
            Some(MeshRef::Asset(project.sub(SHIP, mesh))),
            "{node} shows its primitive"
        );
        let held = match world.get_component::<MaterialRef>(entity) {
            Some(MaterialRef::Asset(uuid)) => *uuid,
            other => panic!("{node} wears {}, not the file's material", worn(other)),
        };
        assert_eq!(held, project.sub(SHIP, material), "{node}'s own material");
        assert_color(&decoded_material(&project, held), color, node);
    }
    let hull = named(&world, "Hull");
    let at = world
        .get_component::<Transform>(hull)
        .expect("Hull is placed")
        .translation;
    assert!(
        (at.x - 3.0).abs() < 1e-5 && at.y.abs() < 1e-5 && at.z.abs() < 1e-5,
        "Hull keeps the file's placement: {at:?}"
    );
}

/// A glTF scene dropped on a row of the hierarchy hangs under it.
#[test]
fn a_gltf_scene_dropped_on_a_row_hangs_under_it() {
    let project = ship_project("spawn-scene-under-row");
    let mut world = GameWorld::new();
    let row = target(&mut world);
    let state = Arc::new(Mutex::new(EditorState::default()));
    state.lock().expect("the editor state").pending_prefab_spawn =
        Some((project.sub(SHIP, "scene"), Some(row)));

    process_pending_prefab_spawn(Some(&project.vfs), &mut world, &state);

    let ship = named(&world, "Ship");
    assert_eq!(
        world.get_component::<Parent>(ship).map(|p| p.0),
        Some(row),
        "the instance's root hangs under the row"
    );
}

// ─── Spawning a mesh ────────────────────────────────────────────────────────

/// **A mesh part of a file spawns under its own identity, named after its
/// file and its label**: `ship mesh1/prim0`, where it was dropped.
#[test]
fn spawning_a_mesh_part_names_it_after_its_file_and_label() {
    let project = ship_project("spawn-mesh-part");
    let mesh = project.sub(SHIP, "mesh1/prim0");
    let mut world = GameWorld::new();
    let state = Arc::new(Mutex::new(EditorState::default()));
    state
        .lock()
        .expect("the editor state")
        .pending_spawn_mesh_asset = Some((mesh, [1.0, 2.0, 3.0], None));

    process_pending_spawn_mesh_asset(Some(&project.vfs), &mut world, &state);

    let spawned = named(&world, "ship mesh1/prim0");
    assert_eq!(
        world.get_component::<MeshRef>(spawned).cloned(),
        Some(MeshRef::Asset(mesh))
    );
    let at = world
        .get_component::<Transform>(spawned)
        .expect("placed")
        .translation;
    assert_eq!([at.x, at.y, at.z], [1.0, 2.0, 3.0], "placed where dropped");
    assert!(
        state
            .lock()
            .expect("the editor state")
            .pending_spawn_mesh_asset
            .is_none(),
        "the request is consumed"
    );
}

/// A mesh file spawns under its own identity, named after its file — the
/// OBJ drop as it always was, by identity now.
#[test]
fn spawning_a_mesh_file_names_it_after_its_file() {
    let project = GltfProject::with("spawn-mesh-file", &[(CRATE, TRIANGLE_OBJ)]);
    let mesh = project.uuid(CRATE);
    let mut world = GameWorld::new();
    let row = target(&mut world);
    let state = Arc::new(Mutex::new(EditorState::default()));
    state
        .lock()
        .expect("the editor state")
        .pending_spawn_mesh_asset = Some((mesh, [0.0, 0.0, 0.0], Some(row)));

    process_pending_spawn_mesh_asset(Some(&project.vfs), &mut world, &state);

    let spawned = named(&world, "crate");
    assert_eq!(
        world.get_component::<MeshRef>(spawned).cloned(),
        Some(MeshRef::Asset(mesh))
    );
    assert_eq!(
        world.get_component::<Parent>(spawned).map(|p| p.0),
        Some(row),
        "it hangs under the row it was dropped on"
    );
}

// ─── Assigning a material or a texture ──────────────────────────────────────

/// **A glTF material assigned to an entity is referenced by its identity**:
/// the entity wears `MaterialRef::Asset` of the part.
#[test]
fn assigning_a_gltf_material_references_it() {
    let project = ship_project("assign-material-part");
    let material = project.sub(SHIP, "material1");
    let mut world = GameWorld::new();
    let entity = target(&mut world);
    let state = Arc::new(Mutex::new(EditorState::default()));
    state
        .lock()
        .expect("the editor state")
        .pending_assign_texture = Some((material, entity));

    process_pending_assign_texture(Some(&project.vfs), &mut world, &state);

    match world.get_component::<MaterialRef>(entity) {
        Some(MaterialRef::Asset(uuid)) => assert_eq!(*uuid, material),
        other => panic!("the entity wears {}, not the glTF material", worn(other)),
    }
}

/// A `.kmat` assigned to an entity is referenced by its identity, as it
/// always was.
#[test]
fn assigning_a_material_file_references_it() {
    let project = GltfProject::with("assign-material-file", &[(STEEL, "()")]);
    let material = project.uuid(STEEL);
    let mut world = GameWorld::new();
    let entity = target(&mut world);
    let state = Arc::new(Mutex::new(EditorState::default()));
    state
        .lock()
        .expect("the editor state")
        .pending_assign_texture = Some((material, entity));

    process_pending_assign_texture(Some(&project.vfs), &mut world, &state);

    match world.get_component::<MaterialRef>(entity) {
        Some(MaterialRef::Asset(uuid)) => assert_eq!(*uuid, material),
        other => panic!("the entity wears {}, not the .kmat", worn(other)),
    }
}

/// The base-colour texture of the inline standard material `entity` wears.
fn inline_base_color_texture(world: &GameWorld, entity: EntityId) -> Option<AssetUUID> {
    match world.get_component::<MaterialRef>(entity) {
        Some(MaterialRef::Inline { material, .. }) => {
            material
                .as_any()
                .downcast_ref::<StandardMaterial>()
                .expect("an inline standard material")
                .base_color_texture
        }
        other => panic!("the entity wears {}, not an inline material", worn(other)),
    }
}

/// **A glTF texture assigned to an entity becomes the base colour of an
/// inline standard material**, naming the part by its identity.
#[test]
fn assigning_a_gltf_texture_gives_an_inline_material_on_it() {
    let project = ship_project("assign-texture-part");
    let texture = project.sub(SHIP, "texture0");
    let mut world = GameWorld::new();
    let entity = target(&mut world);
    let state = Arc::new(Mutex::new(EditorState::default()));
    state
        .lock()
        .expect("the editor state")
        .pending_assign_texture = Some((texture, entity));

    process_pending_assign_texture(Some(&project.vfs), &mut world, &state);

    assert_eq!(inline_base_color_texture(&world, entity), Some(texture));
}

/// An image file assigned to an entity becomes the base colour of an inline
/// standard material, as it always did.
#[test]
fn assigning_an_image_file_gives_an_inline_material_on_it() {
    let project = GltfProject::with("assign-texture-file", &[(WOOD, "PNG")]);
    let texture = project.uuid(WOOD);
    let mut world = GameWorld::new();
    let entity = target(&mut world);
    let state = Arc::new(Mutex::new(EditorState::default()));
    state
        .lock()
        .expect("the editor state")
        .pending_assign_texture = Some((texture, entity));

    process_pending_assign_texture(Some(&project.vfs), &mut world, &state);

    assert_eq!(inline_base_color_texture(&world, entity), Some(texture));
}

// ─── A glTF scene instance against its file ────────────────────────────────

/// Spawns `project`'s `SHIP` scene into `world` as one linked instance.
fn spawn_ship(project: &GltfProject, world: &mut GameWorld) {
    let state = Arc::new(Mutex::new(EditorState::default()));
    state.lock().expect("the editor state").pending_prefab_spawn =
        Some((project.sub(SHIP, "scene"), None));
    process_pending_prefab_spawn(Some(&project.vfs), world, &state);
}

/// The material asset `entity` of `world` wears, by identity.
fn material_of(world: &GameWorld, entity: EntityId, what: &str) -> AssetUUID {
    match world.get_component::<MaterialRef>(entity) {
        Some(MaterialRef::Asset(uuid)) => *uuid,
        other => panic!("{what} wears {}, not the file's material", worn(other)),
    }
}

/// **Applying an override to a glTF scene instance never writes over the
/// glTF file**: the file is an import the editor reads, not a prefab file it
/// owns — a prefab file written in its place would destroy the model.
#[test]
fn applying_to_a_gltf_scene_never_writes_over_the_gltf_file() {
    use khora_sdk::editor_ui::PrefabApplyScope;

    let project = ship_project("apply-to-gltf-scene");
    let mut world = GameWorld::new();
    spawn_ship(&project, &mut world);
    let hull = named(&world, "Hull");
    world
        .get_component_mut::<Transform>(hull)
        .expect("Hull is placed")
        .translation
        .y = 9.0;
    let state = Arc::new(Mutex::new(EditorState::default()));
    state.lock().expect("the editor state").pending_prefab_apply = Some((
        hull,
        PrefabApplyScope::Component {
            type_name: "Transform".to_owned(),
        },
    ));

    process_pending_prefab_apply(Some(&project.vfs), &world, &state);

    let on_disk = std::fs::read(project.path(SHIP)).expect("the glTF is still there");
    assert!(
        on_disk == SHIP_GLTF.as_bytes(),
        "the glTF file was rewritten ({} bytes, starting {:?})",
        on_disk.len(),
        String::from_utf8_lossy(&on_disk[..on_disk.len().min(16)])
    );
}

/// **A saved scene holding a glTF instance comes back with each node in its
/// own material**: save, then load into a fresh world — `Hull` and `Mast`
/// return under the scene's root, each wearing its own material of the file,
/// whose colours decode as the file gives them.
#[test]
fn a_saved_gltf_instance_reloads_with_each_node_in_its_own_material() {
    let project = ship_project("save-reload-gltf-instance");
    let mut world = GameWorld::new();
    spawn_ship(&project, &mut world);
    {
        let mut pvfs = project.vfs.lock().expect("the project");
        assert!(
            crate::scene_io::save_scene_in_project(
                &mut pvfs,
                &world,
                std::path::Path::new("scenes/harbour.kscene"),
            ),
            "the scene saves"
        );
    }

    let mut reloaded = GameWorld::new();
    {
        let mut pvfs = project.vfs.lock().expect("the project");
        assert!(
            crate::scene_io::load_scene_in_project(
                &mut pvfs,
                &mut reloaded,
                "scenes/harbour.kscene"
            ),
            "the scene loads"
        );
    }

    let ship = named(&reloaded, "Ship");
    for (node, material, color) in [
        ("Hull", "material0", HULL_COLOR),
        ("Mast", "material1", MAST_COLOR),
    ] {
        let entity = named(&reloaded, node);
        assert_eq!(
            reloaded.get_component::<Parent>(entity).map(|p| p.0),
            Some(ship),
            "{node} hangs under the scene's root again"
        );
        let held = material_of(&reloaded, entity, node);
        assert_eq!(held, project.sub(SHIP, material), "{node}'s own material");
        assert_color(&decoded_material(&project, held), color, node);
    }
}

/// **A glTF renamed through the project keeps its instances**: the scene
/// saved before the rename loads after it with both nodes — the file's
/// identity, and so its parts', survives the rename.
#[test]
fn a_saved_gltf_instance_reloads_after_its_file_is_renamed() {
    let project = ship_project("rename-gltf-instance");
    let mut world = GameWorld::new();
    spawn_ship(&project, &mut world);
    {
        let mut pvfs = project.vfs.lock().expect("the project");
        assert!(crate::scene_io::save_scene_in_project(
            &mut pvfs,
            &world,
            std::path::Path::new("scenes/harbour.kscene"),
        ));
        pvfs.rename_asset(SHIP, "models/boat.gltf")
            .expect("the glTF is renamed");
    }

    let mut reloaded = GameWorld::new();
    {
        let mut pvfs = project.vfs.lock().expect("the project");
        assert!(crate::scene_io::load_scene_in_project(
            &mut pvfs,
            &mut reloaded,
            "scenes/harbour.kscene"
        ));
    }

    for (node, material) in [("Hull", "material0"), ("Mast", "material1")] {
        let entity = named(&reloaded, node);
        assert_eq!(
            material_of(&reloaded, entity, node),
            project.sub("models/boat.gltf", material),
            "{node} keeps its material through the rename"
        );
    }
}

/// **A glTF scene that cannot be decoded spawns nothing**: its buffer file is
/// missing, the spawn fails, and the world holds no half-placed instance.
#[test]
fn a_gltf_scene_that_cannot_decode_spawns_nothing() {
    let broken = SHIP_GLTF.replace(
        "data:application/octet-stream;base64,AAAAAAAAAAAAAAAAAACAPwAAAAAAAAAAAAAAAAAAgD8AAAAA",
        "ship.bin",
    );
    let project = GltfProject::with("spawn-broken-scene", &[(SHIP, &broken)]);
    let scene = project.sub(SHIP, "scene");
    assert!(
        project
            .vfs
            .lock()
            .expect("the project")
            .asset_service
            .index()
            .get_metadata(&scene)
            .is_some(),
        "the premise: the scene is listed though its buffer is missing"
    );
    let mut world = GameWorld::new();

    spawn_ship(&project, &mut world);

    assert_eq!(
        world.iter_entities().count(),
        0,
        "nothing of the scene is placed"
    );
}
