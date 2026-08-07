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

//! Command boundary tests.
//!
//! Two things have to hold, and neither is visible from a type signature: the
//! applied order must not depend on which worker finished first, and a write
//! that another write silently discards must be reported.

use super::{CommandBuffer, ComponentName, ScriptValue, WorldCommand, WriteTarget};
use crate::ecs::entity::EntityId;
use crate::math::{Quaternion, Vec3};

fn entity(index: u32) -> EntityId {
    EntityId {
        index,
        generation: 1,
    }
}

fn place(index: u32, x: f32) -> WorldCommand {
    WorldCommand::SetTranslation {
        entity: entity(index),
        value: Vec3::new(x, 0.0, 0.0),
    }
}

fn nudge(index: u32, x: f32) -> WorldCommand {
    WorldCommand::Translate {
        entity: entity(index),
        delta: Vec3::new(x, 0.0, 0.0),
    }
}

#[test]
fn commands_come_back_in_the_order_they_were_queued() {
    let mut buffer = CommandBuffer::new();
    buffer.push(place(1, 1.0));
    buffer.push(place(2, 2.0));
    buffer.push(place(3, 3.0));

    assert_eq!(buffer.len(), 3);
    assert_eq!(
        buffer.as_slice(),
        &[place(1, 1.0), place(2, 2.0), place(3, 3.0)]
    );
}

/// Draining hands over everything and leaves the buffer reusable — it is
/// refilled every frame, so a frame that had to grow it should not pay again.
#[test]
fn draining_empties_the_buffer() {
    let mut buffer = CommandBuffer::new();
    buffer.push(place(1, 1.0));
    buffer.push(nudge(1, 2.0));

    let taken: Vec<_> = buffer.drain().collect();
    assert_eq!(taken.len(), 2);
    assert!(buffer.is_empty());
}

/// **The reporting guarantee.** Last-one-wins is a defensible rule and an awful
/// thing to debug in silence, so the discarded write is named.
#[test]
fn two_scripts_placing_one_entity_is_reported() {
    let mut buffer = CommandBuffer::new();
    buffer.push(place(7, 1.0));
    buffer.push(place(7, 2.0));

    let conflicts = buffer.conflicts();
    assert_eq!(conflicts.len(), 1, "{conflicts:?}");
    assert_eq!(conflicts[0].entity, entity(7));
    assert_eq!(conflicts[0].target, WriteTarget::Translation);
    assert_eq!(conflicts[0].writes, 2);
}

/// And the message says what was lost rather than that something happened —
/// "write conflict" would leave the author to work out which write survived.
#[test]
fn the_conflict_message_names_the_entity_and_the_rule() {
    let mut buffer = CommandBuffer::new();
    buffer.push(place(7, 1.0));
    buffer.push(place(7, 2.0));

    let message = buffer.conflicts()[0].message();
    assert!(message.contains("translation"), "got: {message}");
    assert!(message.contains("last write wins"), "got: {message}");
    assert!(message.contains('7'), "got: {message}");
}

/// **The distinction that earns `Translate` its own variant.** Two scripts
/// nudging one entity compose — nothing is lost, so warning about it would only
/// teach the author to ignore the warning.
#[test]
fn two_scripts_nudging_one_entity_is_not_a_conflict() {
    let mut buffer = CommandBuffer::new();
    buffer.push(nudge(7, 1.0));
    buffer.push(nudge(7, 2.0));
    buffer.push(nudge(7, 3.0));

    assert!(buffer.conflicts().is_empty(), "relative moves accumulate");
}

/// One placement among many nudges is still only one absolute write.
#[test]
fn a_placement_among_nudges_does_not_conflict_with_them() {
    let mut buffer = CommandBuffer::new();
    buffer.push(nudge(7, 1.0));
    buffer.push(place(7, 5.0));
    buffer.push(nudge(7, 2.0));

    assert!(buffer.conflicts().is_empty());
}

#[test]
fn writes_to_different_parts_of_one_entity_do_not_conflict() {
    let mut buffer = CommandBuffer::new();
    buffer.push(place(7, 1.0));
    buffer.push(WorldCommand::SetRotation {
        entity: entity(7),
        value: Quaternion::IDENTITY,
    });
    buffer.push(WorldCommand::SetScale {
        entity: entity(7),
        value: Vec3::ONE,
    });

    assert!(buffer.conflicts().is_empty(), "three parts, three writes");
}

#[test]
fn writes_to_the_same_part_of_different_entities_do_not_conflict() {
    let mut buffer = CommandBuffer::new();
    buffer.push(place(1, 1.0));
    buffer.push(place(2, 1.0));

    assert!(buffer.conflicts().is_empty());
}

#[test]
fn two_writes_to_one_component_are_reported_by_name() {
    let mut buffer = CommandBuffer::new();
    for value in [10, 20] {
        buffer.push(WorldCommand::SetComponent {
            entity: entity(3),
            component: ComponentName::new("Health"),
            value: ScriptValue::Int(value),
        });
    }

    let conflicts = buffer.conflicts();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(
        conflicts[0].target,
        WriteTarget::Component(ComponentName::new("Health"))
    );
    assert!(conflicts[0].message().contains("Health"));
}

#[test]
fn different_components_on_one_entity_do_not_conflict() {
    let mut buffer = CommandBuffer::new();
    buffer.push(WorldCommand::SetComponent {
        entity: entity(3),
        component: "Health".into(),
        value: ScriptValue::Int(10),
    });
    buffer.push(WorldCommand::SetComponent {
        entity: entity(3),
        component: "Ammo".into(),
        value: ScriptValue::Int(30),
    });

    assert!(buffer.conflicts().is_empty());
}

/// Reported in a fixed order, so a log compared across two runs of the same
/// frame does not change shape.
#[test]
fn conflicts_are_reported_in_a_stable_order() {
    let mut buffer = CommandBuffer::new();
    for index in [9, 2, 5] {
        buffer.push(place(index, 1.0));
        buffer.push(place(index, 2.0));
    }

    let indices: Vec<_> = buffer.conflicts().iter().map(|c| c.entity.index).collect();
    assert_eq!(indices, vec![2, 5, 9]);
}

/// Spawn creates rather than overwrites: two spawns are two entities, not a
/// lost write.
#[test]
fn spawning_twice_is_not_a_conflict() {
    let spawn = WorldCommand::Spawn {
        position: Vec3::ZERO,
        rotation: Quaternion::IDENTITY,
        components: vec![("Name".into(), ScriptValue::Str("Explosion".to_owned()))],
    };
    let mut buffer = CommandBuffer::new();
    buffer.push(spawn.clone());
    buffer.push(spawn);

    assert!(buffer.conflicts().is_empty());
    assert_eq!(buffer.len(), 2);
}

/// A spawn has no subject yet — the id does not exist until the boundary
/// applies it, so the command cannot name one.
#[test]
fn a_spawn_names_no_entity() {
    let spawn = WorldCommand::Spawn {
        position: Vec3::ZERO,
        rotation: Quaternion::IDENTITY,
        components: vec![("Name".into(), ScriptValue::Str("Explosion".to_owned()))],
    };
    assert_eq!(spawn.entity(), None);
    assert_eq!(spawn.write_target(), None);
}

#[test]
fn every_other_command_names_its_entity() {
    let commands = [
        place(4, 1.0),
        nudge(4, 1.0),
        WorldCommand::SetRotation {
            entity: entity(4),
            value: Quaternion::IDENTITY,
        },
        WorldCommand::SetScale {
            entity: entity(4),
            value: Vec3::ONE,
        },
        WorldCommand::SetParent {
            entity: entity(4),
            parent: None,
        },
        WorldCommand::Despawn { entity: entity(4) },
        WorldCommand::RemoveComponent {
            entity: entity(4),
            component: "Health".into(),
        },
    ];

    for command in commands {
        assert_eq!(command.entity(), Some(entity(4)), "{command:?}");
    }
}

#[test]
fn a_value_reports_the_type_an_author_would_write() {
    assert_eq!(ScriptValue::Int(1).type_name(), "int");
    assert_eq!(ScriptValue::Float(1.0).type_name(), "float");
    assert_eq!(ScriptValue::Vec3(Vec3::ZERO).type_name(), "Vec3");
    assert_eq!(ScriptValue::Entity(entity(1)).type_name(), "Entity");
    assert_eq!(ScriptValue::Unit.type_name(), "void");
}

/// The accessors answer "is it this shape" without a panic, because the applier
/// meets values a script produced and has to reject the wrong one politely.
#[test]
fn an_accessor_declines_rather_than_panicking_on_the_wrong_shape() {
    let value = ScriptValue::Int(42);
    assert_eq!(value.as_int(), Some(42));
    assert_eq!(value.as_float(), None);
    assert_eq!(value.as_str(), None);
    assert_eq!(value.as_entity(), None);
}

#[test]
fn a_string_and_an_array_read_back_by_reference() {
    let text = ScriptValue::Str("boss".to_owned());
    assert_eq!(text.as_str(), Some("boss"));

    let list = ScriptValue::Array(vec![ScriptValue::Int(1), ScriptValue::Int(2)]);
    assert_eq!(list.as_array().map(<[_]>::len), Some(2));
}
