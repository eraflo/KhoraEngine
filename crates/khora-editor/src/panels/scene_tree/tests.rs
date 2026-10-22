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

//! An asset dropped onto the hierarchy queues the request its kind calls
//! for, naming the asset by its identity — a glTF file standing for its
//! default scene, a part of a file for itself.

use khora_sdk::editor_ui::{AssetEntry, EditorState};
use khora_sdk::khora_core::asset::AssetUUID;
use khora_sdk::prelude::ecs::EntityId;

use super::drop::dispatch_asset_drop;
use crate::tests::gltf_fixture::{file_entry, part_entry};

const SHIP: &str = "models/ship.glb";
const CRATE: &str = "models/crate.obj";
const TURRET: &str = "prefabs/turret.kprefab";

/// The browser's list for a project holding a `.glb` (the file and its
/// parts), an OBJ and a `.kprefab`.
fn entries() -> Vec<AssetEntry> {
    vec![
        file_entry(SHIP, "mesh"),
        part_entry(SHIP, "scene", "prefab"),
        part_entry(SHIP, "scene0", "prefab"),
        part_entry(SHIP, "mesh0/prim0", "mesh"),
        part_entry(SHIP, "mesh1/prim0", "mesh"),
        part_entry(SHIP, "material0", "material"),
        part_entry(SHIP, "material1", "material"),
        part_entry(SHIP, "texture0", "texture"),
        file_entry(CRATE, "mesh"),
        file_entry(TURRET, "prefab"),
    ]
}

fn row() -> EntityId {
    EntityId {
        index: 4,
        generation: 1,
    }
}

/// The editor state listing `entries`.
fn state_with(entries: Vec<AssetEntry>) -> EditorState {
    EditorState {
        asset_entries: entries,
        asset_epoch: 1,
        ..EditorState::default()
    }
}

/// Where `entries` lists the asset `uuid`.
fn index_of(state: &EditorState, uuid: AssetUUID) -> usize {
    state
        .asset_entries
        .iter()
        .position(|entry| entry.uuid == uuid)
        .expect("the asset is listed")
}

fn ship() -> AssetUUID {
    AssetUUID::new_v5(SHIP)
}

fn part(label: &str) -> AssetUUID {
    AssetUUID::sub_asset(ship(), label)
}

/// **A glTF file dropped onto the hierarchy spawns its default scene**: a
/// prefab instance of the file's `scene` part, at the root.
#[test]
fn a_gltf_file_dropped_on_the_hierarchy_spawns_its_scene() {
    let mut state = state_with(entries());
    let idx = index_of(&state, ship());

    dispatch_asset_drop(&mut state, idx, None);

    assert_eq!(state.pending_prefab_spawn, Some((part("scene"), None)));
    assert_eq!(state.pending_spawn_mesh_asset, None, "no single-mesh spawn");
}

/// A glTF file dropped onto a row spawns its scene under that row.
#[test]
fn a_gltf_file_dropped_on_a_row_spawns_its_scene_under_it() {
    let mut state = state_with(entries());
    let idx = index_of(&state, ship());

    dispatch_asset_drop(&mut state, idx, Some(row()));

    assert_eq!(
        state.pending_prefab_spawn,
        Some((part("scene"), Some(row())))
    );
}

/// A glTF file whose scene the index does not list — its parts could not be
/// read — still spawns, as one mesh of its own identity.
#[test]
fn a_gltf_file_without_its_scene_listed_spawns_as_one_mesh() {
    let mut state = state_with(vec![file_entry(SHIP, "mesh")]);

    dispatch_asset_drop(&mut state, 0, None);

    assert_eq!(
        state.pending_spawn_mesh_asset,
        Some((ship(), [0.0, 0.0, 0.0], None))
    );
    assert_eq!(state.pending_prefab_spawn, None);
}

/// **An OBJ still spawns as one mesh**, of its own identity.
#[test]
fn an_obj_dropped_on_the_hierarchy_spawns_its_mesh() {
    let mut state = state_with(entries());
    let crate_mesh = AssetUUID::new_v5(CRATE);
    let idx = index_of(&state, crate_mesh);

    dispatch_asset_drop(&mut state, idx, Some(row()));

    assert_eq!(
        state.pending_spawn_mesh_asset,
        Some((crate_mesh, [0.0, 0.0, 0.0], Some(row())))
    );
    assert_eq!(state.pending_prefab_spawn, None);
}

/// **A mesh part of a file spawns as that mesh**, by its own identity.
#[test]
fn a_mesh_part_dropped_on_the_hierarchy_spawns_that_mesh() {
    let mut state = state_with(entries());
    let idx = index_of(&state, part("mesh1/prim0"));

    dispatch_asset_drop(&mut state, idx, None);

    assert_eq!(
        state.pending_spawn_mesh_asset,
        Some((part("mesh1/prim0"), [0.0, 0.0, 0.0], None))
    );
    assert_eq!(state.pending_prefab_spawn, None);
}

/// A scene part of a file spawns as that scene.
#[test]
fn a_scene_part_dropped_on_the_hierarchy_spawns_that_scene() {
    let mut state = state_with(entries());
    let idx = index_of(&state, part("scene0"));

    dispatch_asset_drop(&mut state, idx, None);

    assert_eq!(state.pending_prefab_spawn, Some((part("scene0"), None)));
}

/// A `.kprefab` spawns by its own identity.
#[test]
fn a_prefab_file_dropped_on_the_hierarchy_spawns_it() {
    let mut state = state_with(entries());
    let turret = AssetUUID::new_v5(TURRET);
    let idx = index_of(&state, turret);

    dispatch_asset_drop(&mut state, idx, Some(row()));

    assert_eq!(state.pending_prefab_spawn, Some((turret, Some(row()))));
}

/// **A material part dropped on a row is assigned to that row's entity**,
/// by its own identity.
#[test]
fn a_material_part_dropped_on_a_row_is_assigned_to_it() {
    let mut state = state_with(entries());
    let idx = index_of(&state, part("material1"));

    dispatch_asset_drop(&mut state, idx, Some(row()));

    assert_eq!(
        state.pending_assign_texture,
        Some((part("material1"), row()))
    );
}

/// A texture part dropped on empty space is assigned to the selection.
#[test]
fn a_texture_part_dropped_beside_the_rows_is_assigned_to_the_selection() {
    let mut state = state_with(entries());
    let selected = EntityId {
        index: 9,
        generation: 2,
    };
    state.select(selected);
    let idx = index_of(&state, part("texture0"));

    dispatch_asset_drop(&mut state, idx, None);

    assert_eq!(
        state.pending_assign_texture,
        Some((part("texture0"), selected))
    );
}
