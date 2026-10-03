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

//! A whole world, through every encoding and back.

use khora_core::asset::{AssetUUID, StandardMaterial};
use khora_core::math::LinearRgba;
use khora_data::ecs::{MaterialRef, Transform, World};
use khora_data::scene::{capture_world, TextEncoding};

use super::sample::sample_world;
use super::*;

/// Test-local components, which the sample world does not carry on purpose.
const TEST_COMPONENTS: &[&str] = &["Beacon", "Stamina"];

/// A save is only as good as its worst component: every kind a scene records
/// — all of them, the script with entities in its fields included — must
/// come back from every encoding equal, under the
/// identity it was saved with, with nothing the load had to adapt.
#[test]
fn a_world_round_trips_through_every_encoding() {
    let (mut src, sample) = sample_world();

    // The sample must really hold every kind, or this test proves less than
    // its name says.
    for reg in saved_registrations() {
        if TEST_COMPONENTS.contains(&reg.type_name) {
            continue;
        }
        assert!(
            src.iter_entities()
                .any(|entity| (reg.to_json)(&src, entity).is_some()),
            "the sample world carries no `{}`",
            reg.type_name
        );
    }

    // Both identity namespaces: what an author made, what the game spawned.
    for entity in [sample.root, sample.child, sample.grandchild, sample.panel] {
        src.mark_authored(entity)
            .expect("a live entity can be authored");
    }

    for (name, encoding) in every_encoding() {
        let (dst, applied) = reload(&src, name, encoding);
        assert!(
            applied.report.is_clean(),
            "{name}: a same-version round trip adapted something: {:?}",
            applied.report.entries
        );
        assert_eq!(
            applied.entities.len(),
            src.iter_entities().count(),
            "{name}: every recorded entity is reported as brought in"
        );
        for (id, entity) in &applied.entities {
            assert_eq!(dst.persistent_id(*entity), Some(*id), "{name}");
        }
        assert_same_world(&src, &dst, name);
    }
}

/// A scene holds what an author or a tool wrote, and nothing the engine
/// computed or observed: `GlobalTransform` and `Children` are rebuilt on load,
/// and `BodyMotion` belongs to a running simulation.
#[test]
fn a_capture_holds_only_what_an_author_wrote() {
    let (src, sample) = sample_world();
    let record = capture_world(&src).expect("the sample captures");

    let names: Vec<&str> = record
        .pages
        .iter()
        .flat_map(|page| page.components.iter().map(String::as_str))
        .collect();
    for engine_written in ["GlobalTransform", "Children", "BodyMotion"] {
        assert!(
            !names.contains(&engine_written),
            "`{engine_written}` is the engine's, not the author's"
        );
    }
    for authored in ["Transform", "Parent", "Script", "MaterialRef", "MeshRef"] {
        assert!(names.contains(&authored), "`{authored}` must be recorded");
    }

    for entity in [sample.root, sample.guard, sample.body] {
        let id = src.persistent_id(entity).expect("a live entity has an id");
        assert!(
            record.entities.contains(&id),
            "{entity:?} is recorded under its id"
        );
    }
}

/// An inline material is recorded field by field, by name — readable in the
/// text form, and read back by name into the same material, with the same
/// content identity.
#[test]
fn material_inline_round_trips_by_name() {
    let wall = AssetUUID::new_v5("textures/wall.png");
    let mut src = World::new();
    let entity = src.spawn((
        Transform::identity(),
        MaterialRef::inline(Box::new(StandardMaterial {
            base_color: LinearRgba::new(0.8, 0.1, 0.1, 1.0),
            base_color_texture: Some(wall),
            metallic: 0.25,
            roughness: 0.6,
            ..StandardMaterial::default()
        })),
    ));
    src.mark_authored(entity).expect("authored");
    let identity = src.get::<MaterialRef>(entity).expect("material").uuid();

    let record = capture_world(&src).expect("captures");
    let text = TextEncoding.encode(&record).expect("text encodes");
    let text = String::from_utf8(text).expect("UTF-8");
    for name in [
        "StandardMaterial",
        "\"metallic\"",
        "\"roughness\"",
        "\"base_color\"",
        "$asset",
    ] {
        assert!(text.contains(name), "`{name}` missing from:\n{text}");
    }

    for (name, encoding) in every_encoding() {
        let (dst, applied) = reload(&src, name, encoding);
        assert!(applied.report.is_clean(), "{name}: {:?}", applied.report);
        assert_same_world(&src, &dst, name);
        let twin = dst
            .entity_with_id(src.persistent_id(entity).expect("id"))
            .expect("reloaded");
        assert_eq!(
            dst.get::<MaterialRef>(twin).expect("material").uuid(),
            identity,
            "{name}: the material's content identity changed"
        );
    }
}
