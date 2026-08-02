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

//! Applier tests.
//!
//! The decisive one for this phase: a command moves an entity and the `World`
//! reflects it. The rest guard the ways a faulty script must fail — loudly, in
//! one place, without taking the frame's other work with it.

use khora_core::ecs::entity::EntityId;
use khora_core::math::{Quaternion, Vec3};
use khora_core::script::{CommandBuffer, ScriptValue, WorldCommand};

use super::{apply::apply, apply_all, ApplyError};
use crate::ecs::{Children, Name, Parent, Transform, World};

fn world_with_one_entity() -> (World, EntityId) {
    let mut world = World::new();
    let entity = world.spawn(Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)));
    (world, entity)
}

fn translation(world: &World, entity: EntityId) -> Vec3 {
    world
        .get::<Transform>(entity)
        .expect("the entity has a Transform")
        .translation
}

/// **The milestone for this phase.** A script asks, the engine applies, the
/// `World` shows it — without the lane ever holding a `&mut World`.
#[test]
fn a_command_moves_an_entity_and_the_world_reflects_it() {
    let (mut world, entity) = world_with_one_entity();

    let mut buffer = CommandBuffer::new();
    buffer.push(WorldCommand::SetTranslation {
        entity,
        value: Vec3::new(10.0, 0.0, 0.0),
    });

    assert_eq!(apply_all(&mut world, &buffer), 1);
    assert_eq!(translation(&world, entity), Vec3::new(10.0, 0.0, 0.0));
}

/// Relative moves compose, which is the behavior that earns `Translate` its own
/// variant — and the reason two of them are not reported as a lost write.
#[test]
fn relative_moves_accumulate() {
    let (mut world, entity) = world_with_one_entity();

    let mut buffer = CommandBuffer::new();
    for _ in 0..3 {
        buffer.push(WorldCommand::Translate {
            entity,
            delta: Vec3::new(1.0, 0.0, 0.0),
        });
    }

    assert_eq!(apply_all(&mut world, &buffer), 3);
    assert_eq!(translation(&world, entity), Vec3::new(4.0, 2.0, 3.0));
}

/// Emission order is the contract. Grouping value writes ahead of structural
/// ones would run this batch backwards and drop the write.
#[test]
fn commands_apply_in_emission_order() {
    let (mut world, entity) = world_with_one_entity();

    let mut buffer = CommandBuffer::new();
    buffer.push(WorldCommand::AddComponent {
        entity,
        component: "Name".into(),
        value: ScriptValue::Unit,
    });
    buffer.push(WorldCommand::SetComponent {
        entity,
        component: "Name".into(),
        value: ScriptValue::Str("Boss".to_owned()),
    });

    assert_eq!(apply_all(&mut world, &buffer), 2);
    assert_eq!(
        world.get::<Name>(entity).map(|n| n.0.as_str()),
        Some("Boss")
    );
}

/// **One bad command is one bad command.** A behavior writing to a stale handle
/// must not silently drop every other behavior's work that frame.
#[test]
fn a_failing_command_does_not_stop_the_batch() {
    let (mut world, entity) = world_with_one_entity();
    let ghost = EntityId {
        index: 999,
        generation: 1,
    };

    let mut buffer = CommandBuffer::new();
    buffer.push(WorldCommand::SetTranslation {
        entity: ghost,
        value: Vec3::ONE,
    });
    buffer.push(WorldCommand::SetTranslation {
        entity,
        value: Vec3::new(5.0, 0.0, 0.0),
    });

    assert_eq!(apply_all(&mut world, &buffer), 1, "one of the two applied");
    assert_eq!(translation(&world, entity), Vec3::new(5.0, 0.0, 0.0));
}

/// A recycled index makes an old handle name a different entity. The generation
/// check is what stops the write landing on the stranger.
#[test]
fn a_handle_to_a_despawned_entity_is_refused() {
    let (mut world, entity) = world_with_one_entity();
    world.despawn(entity);

    let error = apply(
        &mut world,
        &WorldCommand::SetTranslation {
            entity,
            value: Vec3::ONE,
        },
    )
    .expect_err("the entity is gone");
    assert_eq!(error, ApplyError::NoSuchEntity(entity));
    assert!(error.to_string().contains("no longer exists"));
}

#[test]
fn moving_an_entity_without_a_transform_says_so() {
    let mut world = World::new();
    let entity = world.spawn(Name("bare".to_owned()));

    let error = apply(
        &mut world,
        &WorldCommand::SetTranslation {
            entity,
            value: Vec3::ONE,
        },
    )
    .expect_err("nothing to move");
    assert_eq!(error, ApplyError::NoTransform(entity));
}

#[test]
fn rotation_and_scale_are_written_too() {
    let (mut world, entity) = world_with_one_entity();
    let turn = Quaternion::new(0.0, 1.0, 0.0, 0.0);

    apply(
        &mut world,
        &WorldCommand::SetRotation {
            entity,
            value: turn,
        },
    )
    .expect("applies");
    apply(
        &mut world,
        &WorldCommand::SetScale {
            entity,
            value: Vec3::new(2.0, 2.0, 2.0),
        },
    )
    .expect("applies");

    let transform = world.get::<Transform>(entity).expect("still there");
    assert_eq!(transform.rotation, turn);
    assert_eq!(transform.scale, Vec3::new(2.0, 2.0, 2.0));
}

// ─── Hierarchy ──────────────────────────────────────────────────────────────

#[test]
fn parenting_maintains_both_halves_of_the_edge() {
    let (mut world, child) = world_with_one_entity();
    let parent = world.spawn(Transform::identity());

    apply(
        &mut world,
        &WorldCommand::SetParent {
            entity: child,
            parent: Some(parent),
        },
    )
    .expect("applies");

    assert_eq!(world.get::<Parent>(child).map(|p| p.0), Some(parent));
    assert_eq!(
        world.get::<Children>(parent).map(|c| c.0.clone()),
        Some(vec![child])
    );
}

/// A cycle would hang every consumer that walks the hierarchy, so the command
/// is refused with a message rather than creating the loop.
#[test]
fn parenting_to_a_descendant_is_refused_and_named() {
    let (mut world, root) = world_with_one_entity();
    let child = world.spawn(Transform::identity());

    apply(
        &mut world,
        &WorldCommand::SetParent {
            entity: child,
            parent: Some(root),
        },
    )
    .expect("applies");

    let error = apply(
        &mut world,
        &WorldCommand::SetParent {
            entity: root,
            parent: Some(child),
        },
    )
    .expect_err("that edge would close a loop");

    assert_eq!(
        error,
        ApplyError::WouldCycle {
            entity: root,
            parent: child,
        }
    );
    assert!(error.to_string().contains("already above it"));
}

#[test]
fn parenting_to_a_dead_entity_is_refused() {
    let (mut world, child) = world_with_one_entity();
    let parent = world.spawn(Transform::identity());
    world.despawn(parent);

    let error = apply(
        &mut world,
        &WorldCommand::SetParent {
            entity: child,
            parent: Some(parent),
        },
    )
    .expect_err("the parent is gone");
    assert_eq!(error, ApplyError::NoSuchEntity(parent));
}

// ─── Lifecycle ──────────────────────────────────────────────────────────────

/// Despawn is recursive — the invariant itself is covered by the `World`
/// hierarchy tests; this one proves the command reaches it.
#[test]
fn despawning_takes_the_whole_subtree() {
    let mut world = World::new();
    let root = world.spawn(Transform::identity());
    let child = world.spawn(Transform::identity());

    apply(
        &mut world,
        &WorldCommand::SetParent {
            entity: child,
            parent: Some(root),
        },
    )
    .expect("applies");
    apply(&mut world, &WorldCommand::Despawn { entity: root }).expect("applies");

    for entity in [root, child] {
        assert!(!world.contains(entity), "{entity:?} survived");
    }
}

#[test]
fn despawning_something_already_gone_is_reported_once() {
    let (mut world, entity) = world_with_one_entity();
    apply(&mut world, &WorldCommand::Despawn { entity }).expect("applies");

    let error =
        apply(&mut world, &WorldCommand::Despawn { entity }).expect_err("already despawned");
    assert_eq!(error, ApplyError::NoSuchEntity(entity));
}

#[test]
fn spawning_places_the_entity_and_attaches_its_components() {
    let mut world = World::new();

    apply(
        &mut world,
        &WorldCommand::Spawn {
            position: Vec3::new(4.0, 5.0, 6.0),
            rotation: Quaternion::IDENTITY,
            components: vec![("Name".into(), ScriptValue::Str("Explosion".to_owned()))],
        },
    )
    .expect("applies");

    let spawned: Vec<_> = world.iter_entities().collect();
    assert_eq!(spawned.len(), 1);
    assert_eq!(translation(&world, spawned[0]), Vec3::new(4.0, 5.0, 6.0));
    assert_eq!(
        world.get::<Name>(spawned[0]).map(|n| n.0.as_str()),
        Some("Explosion")
    );
}

/// A projectile with no damage is harder to notice than one that never
/// appeared, so a half-built entity is rolled back rather than left in the
/// scene.
#[test]
fn a_spawn_whose_component_fails_leaves_nothing_behind() {
    let mut world = World::new();

    apply(
        &mut world,
        &WorldCommand::Spawn {
            position: Vec3::ZERO,
            rotation: Quaternion::IDENTITY,
            components: vec![
                ("Name".into(), ScriptValue::Str("Explosion".to_owned())),
                ("Wobble".into(), ScriptValue::Int(1)),
            ],
        },
    )
    .expect_err("Wobble is not a component");

    assert_eq!(world.iter_entities().count(), 0, "nothing was left behind");
}

// ─── Components ─────────────────────────────────────────────────────────────

/// **The merge.** `health.current = 50` names one field; the others must keep
/// their values rather than reset because the script did not mention them.
#[test]
fn writing_one_field_leaves_the_others_alone() {
    let mut world = World::new();
    let turn = Quaternion::new(0.0, 1.0, 0.0, 0.0);
    let entity = world.spawn(Transform::new(
        Vec3::new(1.0, 2.0, 3.0),
        turn,
        Vec3::new(2.0, 2.0, 2.0),
    ));

    apply(
        &mut world,
        &WorldCommand::SetComponent {
            entity,
            component: "Transform".into(),
            value: ScriptValue::Struct(vec![(
                "translation".to_owned(),
                ScriptValue::Vec3(Vec3::new(9.0, 9.0, 9.0)),
            )]),
        },
    )
    .expect("applies");

    let transform = world.get::<Transform>(entity).expect("still there");
    assert_eq!(transform.translation, Vec3::new(9.0, 9.0, 9.0));
    assert_eq!(transform.rotation, turn, "untouched fields survive");
    assert_eq!(transform.scale, Vec3::new(2.0, 2.0, 2.0));
}

#[test]
fn adding_a_component_then_writing_to_it_works() {
    let (mut world, entity) = world_with_one_entity();

    apply(
        &mut world,
        &WorldCommand::AddComponent {
            entity,
            component: "Name".into(),
            value: ScriptValue::Str("Guard".to_owned()),
        },
    )
    .expect("applies");

    assert_eq!(
        world.get::<Name>(entity).map(|n| n.0.as_str()),
        Some("Guard")
    );
}

/// Adding what is already there would reset it to defaults, which is never what
/// the author meant by "add".
#[test]
fn adding_a_component_twice_is_refused() {
    let (mut world, entity) = world_with_one_entity();
    let add = WorldCommand::AddComponent {
        entity,
        component: "Name".into(),
        value: ScriptValue::Str("Guard".to_owned()),
    };

    apply(&mut world, &add).expect("applies");
    let error = apply(&mut world, &add).expect_err("already attached");
    assert_eq!(
        error,
        ApplyError::AlreadyAttached {
            entity,
            component: "Name".to_owned(),
        }
    );
}

/// Writing to something the entity does not have is a mistake worth naming —
/// creating it silently would hide the typo that caused it.
#[test]
fn writing_to_an_absent_component_says_to_add_it_first() {
    let (mut world, entity) = world_with_one_entity();

    let error = apply(
        &mut world,
        &WorldCommand::SetComponent {
            entity,
            component: "Name".into(),
            value: ScriptValue::Str("Guard".to_owned()),
        },
    )
    .expect_err("not attached");

    assert!(matches!(error, ApplyError::NotAttached { .. }));
    assert!(error.to_string().contains("add it before writing"));
}

#[test]
fn removing_a_component_detaches_it() {
    let (mut world, entity) = world_with_one_entity();
    apply(
        &mut world,
        &WorldCommand::AddComponent {
            entity,
            component: "Name".into(),
            value: ScriptValue::Str("Guard".to_owned()),
        },
    )
    .expect("applies");

    apply(
        &mut world,
        &WorldCommand::RemoveComponent {
            entity,
            component: "Name".into(),
        },
    )
    .expect("applies");

    assert!(world.get::<Name>(entity).is_none());
}

#[test]
fn an_unknown_component_is_named_in_the_error() {
    let (mut world, entity) = world_with_one_entity();

    let error = apply(
        &mut world,
        &WorldCommand::SetComponent {
            entity,
            component: "Wobble".into(),
            value: ScriptValue::Int(1),
        },
    )
    .expect_err("no such type");

    assert_eq!(error, ApplyError::UnknownComponent("Wobble".to_owned()));
    assert!(error.to_string().contains("Wobble"));
}

/// A value of the wrong shape is refused by the component's own mirror, and the
/// message carries which component said no.
#[test]
fn a_value_of_the_wrong_shape_is_refused() {
    let (mut world, entity) = world_with_one_entity();

    let error = apply(
        &mut world,
        &WorldCommand::SetComponent {
            entity,
            component: "Transform".into(),
            value: ScriptValue::Int(42),
        },
    )
    .expect_err("a Transform is not an integer");

    assert!(matches!(error, ApplyError::Rejected { .. }));
    assert!(error.to_string().contains("Transform"));
}

/// JSON has no NaN, and a script can make one from `0.0 / 0.0`. Converting it
/// silently would write a missing field where a number belongs.
#[test]
fn a_non_finite_number_is_refused_rather_than_written_as_nothing() {
    let (mut world, entity) = world_with_one_entity();

    let error = apply(
        &mut world,
        &WorldCommand::SetComponent {
            entity,
            component: "Transform".into(),
            value: ScriptValue::Struct(vec![(
                "translation".to_owned(),
                ScriptValue::Float(f32::NAN),
            )]),
        },
    )
    .expect_err("NaN cannot be written");

    assert!(matches!(error, ApplyError::Rejected { .. }));
    assert_eq!(
        translation(&world, entity),
        Vec3::new(1.0, 2.0, 3.0),
        "and nothing was written"
    );
}

// ─── Value conversion ───────────────────────────────────────────────────────

mod json {
    use super::super::json::{merge, to_json};
    use khora_core::script::ScriptValue;
    use serde_json::json;

    #[test]
    fn scalars_convert_to_their_json_shape() {
        assert_eq!(to_json(&ScriptValue::Int(7)), Ok(json!(7)));
        assert_eq!(to_json(&ScriptValue::Bool(true)), Ok(json!(true)));
        assert_eq!(to_json(&ScriptValue::Unit), Ok(json!(null)));
        assert_eq!(to_json(&ScriptValue::Str("hi".to_owned())), Ok(json!("hi")));
    }

    #[test]
    fn a_struct_converts_to_an_object() {
        let value = ScriptValue::Struct(vec![
            ("current".to_owned(), ScriptValue::Int(50)),
            ("max".to_owned(), ScriptValue::Int(100)),
        ]);
        assert_eq!(to_json(&value), Ok(json!({"current": 50, "max": 100})));
    }

    #[test]
    fn an_infinite_float_is_refused() {
        assert!(to_json(&ScriptValue::Float(f32::INFINITY)).is_err());
        assert!(to_json(&ScriptValue::Float(f32::NAN)).is_err());
        assert!(to_json(&ScriptValue::Float(1.5)).is_ok());
    }

    /// The refusal reaches inside a struct too — a NaN nested one level deep is
    /// no more writable than one at the top.
    #[test]
    fn a_refusal_propagates_out_of_a_nested_value() {
        let value = ScriptValue::Struct(vec![(
            "position".to_owned(),
            ScriptValue::Array(vec![ScriptValue::Float(f32::NAN)]),
        )]);
        assert!(to_json(&value).is_err());
    }

    #[test]
    fn merging_keeps_the_fields_the_patch_does_not_mention() {
        let base = json!({"current": 100, "max": 100});
        let patch = json!({"current": 50});
        assert_eq!(merge(base, patch), json!({"current": 50, "max": 100}));
    }

    #[test]
    fn merging_reaches_into_nested_objects() {
        let base = json!({"transform": {"x": 1, "y": 2}, "name": "a"});
        let patch = json!({"transform": {"y": 9}});
        assert_eq!(
            merge(base, patch),
            json!({"transform": {"x": 1, "y": 9}, "name": "a"})
        );
    }

    /// An array is replaced whole. Merging element-wise would make
    /// `waypoints = [a, b]` on a four-element list keep the last two.
    #[test]
    fn merging_replaces_arrays_rather_than_splicing_them() {
        let base = json!({"waypoints": [1, 2, 3, 4]});
        let patch = json!({"waypoints": [9, 8]});
        assert_eq!(merge(base, patch), json!({"waypoints": [9, 8]}));
    }
}
