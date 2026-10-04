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

//! What saving and opening a scene costs as it holds more prefab instances.

use std::time::{Duration, Instant};

use khora_core::asset::AssetUUID;
use khora_data::ecs::{Name, Transform};
use khora_data::scene::{capture_subtree, instantiate_prefab};

use super::prefab_sample::*;
use super::*;

/// A prefab of ten entities: a root and nine parts under it.
fn ten_part_prefab() -> SceneRecord {
    let mut world = World::new();
    let root = world.spawn((Transform::identity(), Name::new("Root")));
    world.mark_authored(root).expect("authored");
    for n in 0..9 {
        let part = world.spawn((Transform::identity(), Name::new(format!("Part {n}"))));
        world.set_parent(part, Some(root));
        world.mark_authored(part).expect("authored");
    }
    capture_subtree(&world, root).expect("the prefab captures")
}

/// How long a scene of `count` instances of one ten-entity prefab takes to
/// save (collapse) and to open again (expand).
fn save_and_open(count: usize) -> Duration {
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, ten_part_prefab());
    let mut world = World::new();
    for _ in 0..count {
        instantiate_prefab(&mut world, prefab, &library).expect("instantiates");
    }
    let start = Instant::now();
    let scene = save(&world, &library);
    let opened = open(&scene, &library);
    let spent = start.elapsed();
    assert_eq!(opened.iter_entities().count(), count * 10);
    spent
}

/// A level placing four times as many instances costs about four times as
/// much to save and open — not sixteen: each instance's merge reads its own
/// entities, not every entity of the scene around it.
#[test]
fn saving_and_opening_scale_linearly_with_the_instances_a_scene_holds() {
    let few = save_and_open(50);
    let many = save_and_open(200);
    let ratio = many.as_secs_f64() / few.as_secs_f64().max(1e-6);
    assert!(
        ratio < 8.0,
        "4x the instances cost {ratio:.1}x the time ({few:?} for 50, {many:?} for 200)"
    );
}
