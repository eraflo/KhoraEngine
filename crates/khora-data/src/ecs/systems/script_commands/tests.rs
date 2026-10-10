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
    use super::super::json::to_json;
    use khora_core::script::ScriptValue;
    use serde_json::json;

    #[test]
    fn scalars_convert_to_their_json_shape() {
        assert_eq!(to_json(&ScriptValue::Int(7)), Ok(json!(7)));
        assert_eq!(to_json(&ScriptValue::Bool(true)), Ok(json!(true)));
        assert_eq!(to_json(&ScriptValue::Unit), Ok(json!(null)));
        assert_eq!(to_json(&ScriptValue::Str("hi".to_owned())), Ok(json!("hi")));
    }

    /// The absent optional is JSON's `null` — an `Option` field of the mirror
    /// reads it as `None`.
    #[test]
    fn null_converts_to_json_null() {
        assert_eq!(to_json(&ScriptValue::Null), Ok(json!(null)));
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
}

// ─── Typed writes ───────────────────────────────────────────────────────────

/// A script's component writes go field by field, through the component's
/// typed access — no serde, no copy of the component — and a declared
/// component takes the same road as a Rust one.
mod typed {
    use std::cell::Cell;

    use khora_core::ecs::entity::EntityId;
    use khora_core::math::{LinearRgba, Quaternion, Vec3};
    use khora_core::script::{ErgonType, FieldValueError, ScriptField, ScriptValue, WorldCommand};

    use super::{apply, ApplyError};
    use crate::ecs::{
        ComponentKey, ComponentProvenance, FieldKind, PackedField, PackedLayout,
        RuntimeComponentDecl, SemanticDomain, Transform, World,
    };

    /// A float that panics when serialised: a component holding one can be
    /// written only by a road that never serialises it.
    #[derive(Debug, Clone, Copy, Default, PartialEq, serde::Deserialize)]
    #[serde(transparent)]
    struct Unserialisable(f32);

    impl serde::Serialize for Unserialisable {
        fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
            panic!("a script write went through serde")
        }
    }

    impl ScriptField for Unserialisable {
        fn ergon() -> ErgonType {
            f32::ergon()
        }
        fn to_script(&self) -> ScriptValue {
            self.0.to_script()
        }
        fn from_script(value: &ScriptValue) -> Result<Self, FieldValueError> {
            f32::from_script(value).map(Self)
        }
    }

    /// A component that cannot be serialised, with a narrow integer, a string
    /// and a skipped field beside it.
    #[derive(Debug, Clone, Default, PartialEq, khora_macros::Component)]
    struct Thermostat {
        reading: Unserialisable,
        level: u8,
        label: String,
        #[component(skip)]
        cache: u32,
    }

    thread_local! {
        /// How many times a `Tally` was cloned on this thread.
        static TALLY_CLONES: Cell<usize> = const { Cell::new(0) };
    }

    /// A component that counts its clones.
    #[derive(Debug, Default, PartialEq, khora_macros::Component)]
    struct Tally {
        count: i64,
    }

    impl Clone for Tally {
        fn clone(&self) -> Self {
            TALLY_CLONES.with(|clones| clones.set(clones.get() + 1));
            Self { count: self.count }
        }
    }

    fn world() -> World {
        let mut world = World::new();
        world.register_component::<Thermostat>(SemanticDomain::Spatial);
        world.register_component::<Tally>(SemanticDomain::Spatial);
        world
    }

    fn thermostat() -> Thermostat {
        Thermostat {
            reading: Unserialisable(1.0),
            level: 3,
            label: "hall".to_owned(),
            cache: 9,
        }
    }

    fn fields(pairs: &[(&str, ScriptValue)]) -> ScriptValue {
        ScriptValue::Struct(
            pairs
                .iter()
                .map(|(name, value)| ((*name).to_owned(), value.clone()))
                .collect(),
        )
    }

    fn set(entity: EntityId, component: &str, value: ScriptValue) -> WorldCommand {
        WorldCommand::SetComponent {
            entity,
            component: component.into(),
            value,
        }
    }

    fn add(entity: EntityId, component: &str, value: ScriptValue) -> WorldCommand {
        WorldCommand::AddComponent {
            entity,
            component: component.into(),
            value,
        }
    }

    fn spawn_with(components: Vec<(&str, ScriptValue)>) -> WorldCommand {
        WorldCommand::Spawn {
            position: Vec3::ZERO,
            rotation: Quaternion::IDENTITY,
            components: components
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect(),
        }
    }

    fn is_rejected_by(error: &ApplyError, name: &str) -> bool {
        matches!(error, ApplyError::Rejected { component, .. } if component == name)
    }

    /// **The JSON road is gone.** `Thermostat` panics if it is ever
    /// serialised; a script sets it, adds it and spawns it without that
    /// happening, and the values land.
    #[test]
    fn a_script_write_does_not_serialise() {
        let mut world = world();
        let entity = world.spawn((Transform::identity(), thermostat()));

        apply(
            &mut world,
            &set(
                entity,
                "Thermostat",
                fields(&[("reading", ScriptValue::Float(2.5))]),
            ),
        )
        .expect("a float reading is written");
        assert_eq!(
            world.get::<Thermostat>(entity).map(|t| t.reading),
            Some(Unserialisable(2.5))
        );

        let bare = world.spawn(Transform::identity());
        apply(
            &mut world,
            &add(
                bare,
                "Thermostat",
                fields(&[("level", ScriptValue::Int(4))]),
            ),
        )
        .expect("a Thermostat is added");
        assert_eq!(
            world.get::<Thermostat>(bare),
            Some(&Thermostat {
                level: 4,
                ..Thermostat::default()
            })
        );

        apply(
            &mut world,
            &spawn_with(vec![(
                "Thermostat",
                fields(&[("label", ScriptValue::Str("porch".to_owned()))]),
            )]),
        )
        .expect("a Thermostat is spawned");
        let porch = world
            .iter_entities()
            .filter_map(|e| world.get::<Thermostat>(e))
            .filter(|t| t.label == "porch")
            .count();
        assert_eq!(porch, 1);
    }

    /// Only the named fields change. The skipped one keeps its value too: a
    /// typed write never rebuilds the component from what a script sees.
    #[test]
    fn a_script_write_touches_only_named_fields() {
        let mut world = world();
        let entity = world.spawn((Transform::identity(), thermostat()));

        apply(
            &mut world,
            &set(
                entity,
                "Thermostat",
                fields(&[("level", ScriptValue::Int(7))]),
            ),
        )
        .expect("a level is written");

        assert_eq!(
            world.get::<Thermostat>(entity),
            Some(&Thermostat {
                level: 7,
                ..thermostat()
            })
        );
    }

    /// **Attached or not is a lookup.** Refusing to add what an entity has
    /// does not copy the component to find out.
    #[test]
    fn attachment_is_decided_without_cloning() {
        let mut world = world();
        let entity = world.spawn((Transform::identity(), Tally { count: 1 }));
        TALLY_CLONES.with(|clones| clones.set(0));

        let error = apply(&mut world, &add(entity, "Tally", ScriptValue::Unit))
            .expect_err("already attached");

        assert_eq!(
            error,
            ApplyError::AlreadyAttached {
                entity,
                component: "Tally".to_owned(),
            }
        );
        assert_eq!(TALLY_CLONES.with(Cell::get), 0, "the Tally was cloned");
        assert_eq!(world.get::<Tally>(entity), Some(&Tally { count: 1 }));
    }

    /// **All or nothing.** A patch with one value that fits and one that does
    /// not is refused whole: neither field changes.
    #[test]
    fn a_refused_value_writes_nothing() {
        let mut world = world();
        let entity = world.spawn((Transform::identity(), thermostat()));

        let error = apply(
            &mut world,
            &set(
                entity,
                "Thermostat",
                fields(&[
                    ("label", ScriptValue::Str("attic".to_owned())),
                    ("level", ScriptValue::Int(300)),
                ]),
            ),
        )
        .expect_err("300 is no u8");

        assert!(is_rejected_by(&error, "Thermostat"), "got {error:?}");
        assert_eq!(world.get::<Thermostat>(entity), Some(&thermostat()));
    }

    /// A NaN reading is not a reading: refused, and a spawn holding one rolls
    /// back whole, as an add holding one attaches nothing.
    #[test]
    fn a_non_finite_float_is_refused() {
        let mut world = world();

        let error = apply(
            &mut world,
            &spawn_with(vec![(
                "Thermostat",
                fields(&[("reading", ScriptValue::Float(f32::NAN))]),
            )]),
        )
        .expect_err("NaN is refused");
        assert!(is_rejected_by(&error, "Thermostat"), "got {error:?}");
        assert_eq!(world.iter_entities().count(), 0, "the spawn rolled back");

        let entity = world.spawn(Transform::identity());
        let error = apply(
            &mut world,
            &add(
                entity,
                "Thermostat",
                fields(&[("reading", ScriptValue::Float(f32::INFINITY))]),
            ),
        )
        .expect_err("infinity is refused");
        assert!(is_rejected_by(&error, "Thermostat"), "got {error:?}");
        assert!(
            world.get::<Thermostat>(entity).is_none(),
            "nothing attached"
        );
    }

    /// A `u8` takes `0..=255`; anything else is refused rather than wrapped.
    #[test]
    fn an_integer_out_of_range_is_refused() {
        let mut world = world();
        let entity = world.spawn((Transform::identity(), thermostat()));

        for value in [-1, 256, 300] {
            let error = apply(
                &mut world,
                &set(
                    entity,
                    "Thermostat",
                    fields(&[("level", ScriptValue::Int(value))]),
                ),
            )
            .expect_err("out of a u8's range");
            assert!(
                is_rejected_by(&error, "Thermostat"),
                "{value}: got {error:?}"
            );
        }
        assert_eq!(world.get::<Thermostat>(entity), Some(&thermostat()));

        apply(
            &mut world,
            &set(
                entity,
                "Thermostat",
                fields(&[("level", ScriptValue::Int(255))]),
            ),
        )
        .expect("255 fits");
        assert_eq!(world.get::<Thermostat>(entity).map(|t| t.level), Some(255));
    }

    /// A skipped field has no slot, so a script cannot write it: the write is
    /// refused rather than silently dropped.
    #[test]
    fn a_skipped_field_is_not_written_by_a_script() {
        let mut world = world();
        let entity = world.spawn((Transform::identity(), thermostat()));

        let error = apply(
            &mut world,
            &set(
                entity,
                "Thermostat",
                fields(&[("cache", ScriptValue::Int(1))]),
            ),
        )
        .expect_err("`cache` is skipped");

        assert!(is_rejected_by(&error, "Thermostat"), "got {error:?}");
        assert_eq!(world.get::<Thermostat>(entity), Some(&thermostat()));
    }

    /// A field the component does not have is refused, naming the component,
    /// rather than ignored.
    #[test]
    fn a_write_to_an_unknown_field_is_rejected() {
        let mut world = world();
        let entity = world.spawn(Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)));

        let error = apply(
            &mut world,
            &set(
                entity,
                "Transform",
                fields(&[("tranlsation", ScriptValue::Vec3(Vec3::ONE))]),
            ),
        )
        .expect_err("Transform has no `tranlsation`");

        assert!(is_rejected_by(&error, "Transform"), "got {error:?}");
        assert_eq!(
            world.get::<Transform>(entity).map(|t| t.translation),
            Some(Vec3::new(1.0, 2.0, 3.0))
        );
    }

    // ─── Declared components ────────────────────────────────────────────────

    /// `Charge { amount: int, tint: Color }`, declared while the engine runs.
    fn declare_charge(world: &mut World) -> ComponentKey {
        let field = |name: &str, kind, default| PackedField {
            name: name.to_owned(),
            kind,
            default,
        };
        world
            .register_runtime_component(RuntimeComponentDecl {
                name: "Charge".to_owned(),
                domain: SemanticDomain::Spatial,
                provenance: ComponentProvenance::Authored,
                layout: PackedLayout::new(vec![
                    field("amount", FieldKind::Int, ScriptValue::Int(0)),
                    field(
                        "tint",
                        FieldKind::Color,
                        ScriptValue::Color(LinearRgba::WHITE),
                    ),
                ])
                .expect("a valid layout"),
            })
            .expect("Charge registers")
    }

    /// `entity`'s value of field `name` of the declared component `key`.
    fn read(world: &World, entity: EntityId, key: ComponentKey, name: &str) -> Option<ScriptValue> {
        let slot = world
            .components()
            .vtable(key)?
            .columns
            .packed()?
            .slot_of(name)?;
        world.row(entity, key)?.field(slot)
    }

    /// **One road.** `SetComponent` naming a declared component writes its
    /// packed fields — the named one only — and refuses a value of the wrong
    /// kind without writing anything.
    #[test]
    fn a_declared_component_is_written_by_the_same_road() {
        let mut world = world();
        let charge = declare_charge(&mut world);
        let entity = world.spawn(Transform::identity());
        world
            .add_runtime_component(entity, charge, &ScriptValue::Unit)
            .expect("Charge attaches");

        apply(
            &mut world,
            &set(entity, "Charge", fields(&[("amount", ScriptValue::Int(7))])),
        )
        .expect("an int amount is written");
        assert_eq!(
            read(&world, entity, charge, "amount"),
            Some(ScriptValue::Int(7))
        );
        assert_eq!(
            read(&world, entity, charge, "tint"),
            Some(ScriptValue::Color(LinearRgba::WHITE)),
            "the field not named is untouched"
        );

        let error = apply(
            &mut world,
            &set(
                entity,
                "Charge",
                fields(&[
                    ("amount", ScriptValue::Int(9)),
                    ("tint", ScriptValue::Int(1)),
                ]),
            ),
        )
        .expect_err("an int is not a Color");
        assert!(is_rejected_by(&error, "Charge"), "got {error:?}");
        assert_eq!(
            read(&world, entity, charge, "amount"),
            Some(ScriptValue::Int(7)),
            "nothing written"
        );
    }

    /// And added, refused when already there, spawned with, and removed — by
    /// name, like a Rust component.
    #[test]
    fn a_declared_component_is_added_and_removed_by_name() {
        let mut world = world();
        let charge = declare_charge(&mut world);
        let entity = world.spawn(Transform::identity());

        apply(
            &mut world,
            &add(entity, "Charge", fields(&[("amount", ScriptValue::Int(2))])),
        )
        .expect("Charge is added");
        assert_eq!(
            read(&world, entity, charge, "amount"),
            Some(ScriptValue::Int(2))
        );

        let error = apply(&mut world, &add(entity, "Charge", ScriptValue::Unit))
            .expect_err("already attached");
        assert_eq!(
            error,
            ApplyError::AlreadyAttached {
                entity,
                component: "Charge".to_owned(),
            }
        );

        apply(
            &mut world,
            &WorldCommand::RemoveComponent {
                entity,
                component: "Charge".into(),
            },
        )
        .expect("Charge is removed");
        assert!(world.row(entity, charge).is_none(), "detached");

        let before: Vec<EntityId> = world.iter_entities().collect();
        apply(
            &mut world,
            &spawn_with(vec![("Charge", fields(&[("amount", ScriptValue::Int(5))]))]),
        )
        .expect("a Charge is spawned");
        let spawned: Vec<EntityId> = world
            .iter_entities()
            .filter(|e| !before.contains(e))
            .collect();
        assert_eq!(spawned.len(), 1);
        assert_eq!(
            read(&world, spawned[0], charge, "amount"),
            Some(ScriptValue::Int(5))
        );
    }

    // ─── What a typed write accepts ─────────────────────────────────────────

    /// **The language's one widening.** `float m = 1;` holds an `Int` at run
    /// time — the VM widens it where a float is read — and `e.Set(RigidBody {
    /// mass: m })` queues that `Int`. The JSON road read it as the float it
    /// means; the typed road must too, rather than refuse a write the checker
    /// accepted.
    #[test]
    fn an_int_written_to_a_float_field_lands_as_that_float() {
        let mut world = World::new();
        let entity = world.spawn((Transform::identity(), crate::ecs::RigidBody::default()));

        apply(
            &mut world,
            &set(
                entity,
                "RigidBody",
                fields(&[("mass", ScriptValue::Int(3))]),
            ),
        )
        .expect("an int mass is the float it means");

        assert_eq!(
            world
                .get::<crate::ecs::RigidBody>(entity)
                .map(|body| body.mass),
            Some(3.0)
        );
    }

    /// A NaN inside a `Vec3` or a `Quat` is no more a position or a turn than
    /// a NaN `f32` is a mass: the JSON road refused one (serde reads no `null`
    /// as an `f32`), and so must the typed road — every system reading the
    /// transform would inherit it.
    #[test]
    fn a_non_finite_component_of_an_engine_value_is_refused() {
        let mut world = World::new();
        let entity = world.spawn(Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)));

        let error = apply(
            &mut world,
            &set(
                entity,
                "Transform",
                fields(&[(
                    "translation",
                    ScriptValue::Vec3(Vec3::new(f32::NAN, 0.0, 0.0)),
                )]),
            ),
        )
        .expect_err("a NaN translation is refused");
        assert!(is_rejected_by(&error, "Transform"), "got {error:?}");

        let error = apply(
            &mut world,
            &set(
                entity,
                "Transform",
                fields(&[(
                    "rotation",
                    ScriptValue::Quat(Quaternion::new(0.0, f32::INFINITY, 0.0, 1.0)),
                )]),
            ),
        )
        .expect_err("an infinite rotation is refused");
        assert!(is_rejected_by(&error, "Transform"), "got {error:?}");

        let transform = world.get::<Transform>(entity).expect("still there");
        assert_eq!(transform.translation, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(transform.rotation, Quaternion::IDENTITY);
    }

    /// **One road, one rule.** A declared component's `float` field refuses a
    /// NaN as a Rust `f32` field does.
    #[test]
    fn a_declared_float_field_refuses_a_non_finite_value() {
        let mut world = world();
        let fuel = world
            .register_runtime_component(RuntimeComponentDecl {
                name: "Fuel".to_owned(),
                domain: SemanticDomain::Spatial,
                provenance: ComponentProvenance::Authored,
                layout: PackedLayout::new(vec![PackedField {
                    name: "litres".to_owned(),
                    kind: FieldKind::Float,
                    default: ScriptValue::Float(5.0),
                }])
                .expect("a valid layout"),
            })
            .expect("Fuel registers");
        let entity = world.spawn(Transform::identity());
        world
            .add_runtime_component(entity, fuel, &ScriptValue::Unit)
            .expect("Fuel attaches");

        let error = apply(
            &mut world,
            &set(
                entity,
                "Fuel",
                fields(&[("litres", ScriptValue::Float(f32::NAN))]),
            ),
        )
        .expect_err("a NaN is refused");

        assert!(is_rejected_by(&error, "Fuel"), "got {error:?}");
        assert_eq!(
            read(&world, entity, fuel, "litres"),
            Some(ScriptValue::Float(5.0))
        );
    }

    /// A route: an array beside a scalar.
    #[derive(Debug, Clone, Default, PartialEq, khora_macros::Component)]
    struct Patrol {
        waypoints: Vec<Vec3>,
        speed: f32,
    }

    /// **An array is replaced whole.** `waypoints = [a, b]` on a four-point
    /// route leaves exactly `[a, b]` — not the first two overwritten and the
    /// last two kept — and the field not named keeps its value.
    #[test]
    fn a_vec_field_set_by_a_script_is_replaced_whole() {
        let mut world = world();
        world.register_component::<Patrol>(SemanticDomain::Spatial);
        let route = |n: usize| {
            (0..n)
                .map(|i| Vec3::new(i as f32, 0.0, 0.0))
                .collect::<Vec<_>>()
        };
        let entity = world.spawn((
            Transform::identity(),
            Patrol {
                waypoints: route(4),
                speed: 2.0,
            },
        ));
        let replaced = [Vec3::new(9.0, 9.0, 9.0), Vec3::new(8.0, 8.0, 8.0)];

        apply(
            &mut world,
            &set(
                entity,
                "Patrol",
                fields(&[(
                    "waypoints",
                    ScriptValue::Array(replaced.iter().copied().map(ScriptValue::Vec3).collect()),
                )]),
            ),
        )
        .expect("an array of Vec3 is written");

        assert_eq!(
            world.get::<Patrol>(entity),
            Some(&Patrol {
                waypoints: replaced.to_vec(),
                speed: 2.0,
            })
        );

        apply(
            &mut world,
            &set(
                entity,
                "Patrol",
                fields(&[("waypoints", ScriptValue::Array(Vec::new()))]),
            ),
        )
        .expect("an empty array is written");
        assert_eq!(
            world.get::<Patrol>(entity).map(|p| p.waypoints.len()),
            Some(0)
        );
    }
}
