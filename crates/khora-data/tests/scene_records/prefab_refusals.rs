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

//! Records that link to prefabs and cannot be loaded as they are: refused
//! whole, never loaded in part.

use khora_core::asset::AssetUUID;
use khora_data::ecs::{Name, Transform};
use khora_data::scene::{
    capture_subtree, collapse, expand, instantiate_prefab, prepare, prepare_game, InstanceRecord,
    NoPrefabs, SaveRecord,
};

use super::prefab_sample::*;
use super::*;

/// A world holding one entity, to check a refusal leaves it as it was.
fn bystander_world() -> (World, PersistentId) {
    let mut world = World::new();
    let bystander = world.spawn((Transform::identity(), Name::new("Bystander")));
    let bystander = world.mark_authored(bystander).expect("authored");
    (world, bystander)
}

fn assert_untouched(world: &World, bystander: PersistentId) {
    assert_eq!(world.iter_entities().count(), 1, "the world changed");
    assert_eq!(name(world, entity(world, bystander)), "Bystander");
}

/// The prefab `holder` — a root with one instance of `inner` under it — as
/// its file holds it: the instance a link.
fn holding(inner: AssetUUID, library: &Library) -> SceneRecord {
    let mut world = World::new();
    let root = world.spawn((Transform::identity(), Name::new("Holder")));
    world.mark_authored(root).expect("authored");
    let nested = instantiate_prefab(&mut world, inner, library).expect("instantiates");
    world.set_parent(nested, Some(root));
    collapse(&capture_subtree(&world, root).expect("captures"), library).expect("collapses")
}

/// A scene whose instance's prefab cannot be found is refused whole: its
/// overrides have nowhere to go, and dropping the instance would lose them at
/// the next save. Instantiating a prefab nobody has is refused the same way,
/// and leaves the world as it was.
#[test]
fn a_missing_prefab_refuses_the_whole_load() {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let mut world = World::new();
    instantiate_prefab(&mut world, prefab, &library).expect("the turret instantiates");
    let scene = save(&world, &library);

    assert!(expand(&scene, &NoPrefabs).is_err(), "no prefab at all");
    assert!(
        expand(
            &scene,
            &Library::default().with(AssetUUID::new(), turret.record)
        )
        .is_err(),
        "a library without that prefab"
    );

    let (mut world, bystander) = bystander_world();
    assert!(instantiate_prefab(&mut world, AssetUUID::new(), &library).is_err());
    assert_untouched(&world, bystander);
}

/// A prefab that holds itself — directly, or through another prefab — has
/// no expansion: it is refused, not followed until the stack runs out.
#[test]
fn a_prefab_cycle_is_refused() {
    let turret = turret();
    let (a, b) = (AssetUUID::new(), AssetUUID::new());
    let library = Library::default().with(a, turret.record.clone());
    let b_holds_a = holding(a, &library);
    let library = library.with(b, b_holds_a.clone());
    let a_holds_b = holding(b, &library);
    let a_holds_a = holding(a, &library);

    let mut world = World::new();
    instantiate_prefab(&mut world, b, &library).expect("the holder instantiates");
    let scene = save(&world, &library);

    // `a` now holds `b`, which holds `a`.
    let through_another = Library::default().with(a, a_holds_b).with(b, b_holds_a);
    assert!(expand(&scene, &through_another).is_err());
    let (mut world, bystander) = bystander_world();
    assert!(instantiate_prefab(&mut world, a, &through_another).is_err());
    assert_untouched(&world, bystander);

    // `a` holds itself.
    let itself = Library::default().with(a, a_holds_a);
    let (mut world, bystander) = bystander_world();
    assert!(instantiate_prefab(&mut world, a, &itself).is_err());
    assert_untouched(&world, bystander);
}

/// A record that still links to prefabs is not a world: loading it as one
/// is refused, nothing reserved, until it is expanded. A game save's
/// composed record is held to the same.
#[test]
fn a_record_that_still_links_to_prefabs_is_refused_by_prepare() {
    let root = PersistentId::authored(0x51);
    let prefab = AssetUUID::new();
    let record = SceneRecord {
        entities: vec![root],
        pages: Vec::new(),
        instances: vec![InstanceRecord {
            root,
            prefab,
            delta: SaveRecord {
                base: prefab,
                order: vec![root],
                destroyed: Vec::new(),
                created: Vec::new(),
                removed: Vec::new(),
                changes: SceneRecord::default(),
                before: SceneRecord::default(),
            },
        }],
    };

    let (mut world, bystander) = bystander_world();
    assert!(prepare(&mut world, &record).is_err(), "prepare");
    assert_untouched(&world, bystander);
    assert!(prepare_game(&mut world, &record).is_err(), "prepare_game");
    assert_untouched(&world, bystander);
    assert!(apply(&mut world, &record, Identity::Keep).is_err(), "apply");
    assert_untouched(&world, bystander);
}
