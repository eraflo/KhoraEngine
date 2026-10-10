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

//! A world whose entities hold components declared at run time: a scene, a
//! snapshot and a game save skip them, and keep the Rust components beside
//! them whole.

use khora_core::asset::AssetUUID;
use khora_core::math::Vec3;
use khora_core::script::ScriptValue;
use khora_data::ecs::{
    ComponentProvenance, FieldKind, PackedField, PackedLayout, RuntimeComponentDecl,
    SemanticDomain, Transform,
};
use khora_data::scene::{capture_save, compose, prepare_game};

use super::snapshot::{load_snapshot, snapshot_of};
use super::*;

/// A world of Transforms, each beside two declared components in its own page.
fn declared_world() -> (World, Vec<EntityId>) {
    let mut world = World::new();
    let mut keys = Vec::new();
    for (name, domain) in [
        ("Glow", SemanticDomain::Spatial),
        ("Mood", SemanticDomain::Script),
    ] {
        let layout = PackedLayout::new(vec![
            PackedField {
                name: "label".to_owned(),
                kind: FieldKind::Value,
                default: ScriptValue::Str("x".to_owned()),
            },
            PackedField {
                name: "n".to_owned(),
                kind: FieldKind::Int,
                default: ScriptValue::Int(3),
            },
        ])
        .expect("a valid layout");
        keys.push(
            world
                .register_runtime_component(RuntimeComponentDecl {
                    name: name.to_owned(),
                    domain,
                    provenance: ComponentProvenance::Authored,
                    layout,
                })
                .expect("registers"),
        );
    }
    let mut entities = Vec::new();
    for i in 0..4 {
        let entity = world.spawn(Transform::from_translation(Vec3::new(i as f32, 1.0, 2.0)));
        if i % 2 == 0 {
            for key in &keys {
                world
                    .add_runtime_component(entity, *key, &ScriptValue::Unit)
                    .expect("attaches");
            }
        }
        world.mark_authored(entity).expect("authored");
        entities.push(entity);
    }
    (world, entities)
}

fn translations(world: &World) -> Vec<f32> {
    let mut xs: Vec<f32> = world
        .query::<&Transform>()
        .map(|t| t.translation.x)
        .collect();
    xs.sort_by(f32::total_cmp);
    xs
}

#[test]
fn a_scene_skips_declared_components_and_keeps_their_neighbours() {
    let (src, _) = declared_world();
    for (name, encoding) in every_encoding() {
        let (dst, applied) = reload(&src, name, encoding);
        assert!(
            applied.report.is_clean(),
            "{name}: {:?}",
            applied.report.entries
        );
        assert_eq!(applied.entities.len(), 4, "{name}");
        assert_eq!(translations(&dst), vec![0.0, 1.0, 2.0, 3.0], "{name}");
        assert!(dst.components().key_named("Glow").is_none(), "{name}");
    }
}

#[test]
fn a_snapshot_skips_declared_components_and_keeps_their_neighbours() {
    let (src, _) = declared_world();
    let file = snapshot_of(&src);
    let mut dst = World::new();
    let applied = load_snapshot(&mut dst, &file).expect("the snapshot loads");
    assert_eq!(applied.entities.len(), 4);
    assert_eq!(translations(&dst), vec![0.0, 1.0, 2.0, 3.0]);
}

#[test]
fn a_game_save_skips_declared_components_and_keeps_their_neighbours() {
    let (src, _) = declared_world();
    let base = capture_world(&src).expect("captures");
    let mut now = src;
    let moved: Vec<EntityId> = now.iter_entities().collect();
    for entity in moved {
        if let Some(t) = now.get_mut::<Transform>(entity) {
            t.translation.y = 5.0;
        }
    }
    let save = capture_save(&now, AssetUUID::new(), &base).expect("saves");
    let mut dst = World::new();
    let prepared = prepare_game(&mut dst, &compose(&base, &save)).expect("loads");
    prepared.commit(&mut dst, Identity::Keep);
    let ys: Vec<f32> = dst.query::<&Transform>().map(|t| t.translation.y).collect();
    assert_eq!(ys, vec![5.0; 4]);
    assert_eq!(translations(&dst), vec![0.0, 1.0, 2.0, 3.0]);
}
