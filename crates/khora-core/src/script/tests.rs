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

use std::sync::Arc;

use super::{
    BatchShapeError, ColumnBatch, CommandBuffer, ComponentName, ScriptValue, WorldCommand,
    WriteTarget,
};
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
    assert!(targets_of(&spawn).is_empty());
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

// ─── Column batches ─────────────────────────────────────────────────────────

/// Every `(entity, target)` a command overwrites, owned, in the order the
/// command names them.
fn targets_of(command: &WorldCommand) -> Vec<(EntityId, WriteTarget)> {
    let mut targets = Vec::new();
    command.for_each_write_target(|entity, target| targets.push((entity, target.into_owned())));
    targets
}

fn names(fields: &[&str]) -> Vec<String> {
    fields.iter().map(|field| (*field).to_owned()).collect()
}

/// A `Health` batch writing `fields`, one row per `(entity, values)`.
fn health_batch(fields: &[&str], rows: &[(u32, Vec<ScriptValue>)]) -> WorldCommand {
    let mut batch = ColumnBatch::new(names(fields)).expect("distinct fields");
    for (index, values) in rows {
        batch
            .push_row(entity(*index), values.clone())
            .expect("a row of the batch's width");
    }
    WorldCommand::WriteColumn {
        component: ComponentName::new("Health"),
        batch: Arc::new(batch),
    }
}

/// `SetComponent(Health, value)` on entity `index`.
fn set_health(index: u32, value: ScriptValue) -> WorldCommand {
    WorldCommand::SetComponent {
        entity: entity(index),
        component: ComponentName::new("Health"),
        value,
    }
}

/// A `Health` patch naming `fields`.
fn health_patch(fields: &[(&str, i64)]) -> ScriptValue {
    ScriptValue::Struct(
        fields
            .iter()
            .map(|(name, value)| ((*name).to_owned(), ScriptValue::Int(*value)))
            .collect(),
    )
}

fn health_field(field: &str) -> WriteTarget {
    WriteTarget::Field(ComponentName::new("Health"), field.to_owned())
}

/// A batch reads back the rows it was given, in the order they were pushed,
/// each with one value per field.
#[test]
fn a_batch_reads_back_its_rows_in_push_order() {
    let mut batch = ColumnBatch::new(names(&["current", "max"])).expect("distinct fields");
    assert!(batch.is_empty());
    assert_eq!(batch.len(), 0);

    batch
        .push_row(entity(4), [ScriptValue::Int(1), ScriptValue::Int(10)])
        .expect("two values for two fields");
    batch
        .push_row(entity(2), vec![ScriptValue::Int(2), ScriptValue::Int(20)])
        .expect("two values for two fields");

    assert_eq!(batch.fields(), &names(&["current", "max"])[..]);
    assert_eq!(batch.len(), 2);
    assert!(!batch.is_empty());
    let rows: Vec<(EntityId, Vec<ScriptValue>)> = batch
        .rows()
        .map(|(entity, values)| (entity, values.to_vec()))
        .collect();
    assert_eq!(
        rows,
        vec![
            (entity(4), vec![ScriptValue::Int(1), ScriptValue::Int(10)]),
            (entity(2), vec![ScriptValue::Int(2), ScriptValue::Int(20)]),
        ]
    );
}

/// The same entity may appear twice: the boundary applies the later row.
#[test]
fn a_batch_keeps_two_rows_of_one_entity() {
    let mut batch = ColumnBatch::new(names(&["current"])).expect("one field");
    batch
        .push_row(entity(1), [ScriptValue::Int(1)])
        .expect("fits");
    batch
        .push_row(entity(1), [ScriptValue::Int(2)])
        .expect("fits");

    assert_eq!(batch.len(), 2);
    let values: Vec<ScriptValue> = batch.rows().map(|(_, values)| values[0].clone()).collect();
    assert_eq!(values, vec![ScriptValue::Int(1), ScriptValue::Int(2)]);
}

/// A row would write a field named twice twice; the batch is refused at
/// `new`, naming the field.
#[test]
fn a_batch_naming_a_field_twice_is_refused() {
    let refused = ColumnBatch::new(names(&["current", "max", "current"]));

    assert_eq!(
        refused,
        Err(BatchShapeError::DuplicateField("current".to_owned()))
    );
}

/// A row holding too few or too many values is refused, and the batch is
/// left as it was.
#[test]
fn a_row_of_the_wrong_width_is_refused() {
    let mut batch = ColumnBatch::new(names(&["current", "max"])).expect("distinct fields");
    batch
        .push_row(entity(1), [ScriptValue::Int(1), ScriptValue::Int(2)])
        .expect("fits");

    assert_eq!(
        batch.push_row(entity(2), [ScriptValue::Int(1)]),
        Err(BatchShapeError::WrongWidth {
            expected: 2,
            found: 1
        })
    );
    assert_eq!(
        batch.push_row(
            entity(3),
            [
                ScriptValue::Int(1),
                ScriptValue::Int(2),
                ScriptValue::Int(3)
            ]
        ),
        Err(BatchShapeError::WrongWidth {
            expected: 2,
            found: 3
        })
    );
    assert_eq!(batch.len(), 1, "a refused row is not kept");
    assert_eq!(
        batch.rows().map(|(entity, _)| entity).collect::<Vec<_>>(),
        vec![entity(1)]
    );
}

/// A batch with no field writes nothing; a row of it holds no value.
#[test]
fn a_batch_of_no_field_takes_empty_rows() {
    let mut batch = ColumnBatch::new(Vec::new()).expect("no field is no duplicate");
    batch
        .push_row(entity(1), Vec::<ScriptValue>::new())
        .expect("zero values for zero fields");
    assert_eq!(
        batch.push_row(entity(2), [ScriptValue::Int(1)]),
        Err(BatchShapeError::WrongWidth {
            expected: 0,
            found: 1
        })
    );
    assert_eq!(batch.len(), 1);
}

/// A column write has many subjects, so it names no single entity.
#[test]
fn a_column_write_names_no_single_entity() {
    let command = health_batch(&["current"], &[(1, vec![ScriptValue::Int(1)])]);

    assert_eq!(command.entity(), None);
}

// ─── Write targets ──────────────────────────────────────────────────────────

/// A batch overwrites one field of one entity per cell.
#[test]
fn a_column_write_targets_every_cell() {
    let command = health_batch(
        &["current", "max"],
        &[
            (1, vec![ScriptValue::Int(1), ScriptValue::Int(10)]),
            (2, vec![ScriptValue::Int(2), ScriptValue::Int(20)]),
        ],
    );

    let mut targets = targets_of(&command);
    targets.sort_by(|a, b| (a.0.index, &a.1).cmp(&(b.0.index, &b.1)));
    assert_eq!(
        targets,
        vec![
            (entity(1), health_field("current")),
            (entity(1), health_field("max")),
            (entity(2), health_field("current")),
            (entity(2), health_field("max")),
        ]
    );
}

/// A `Set` of a struct writes the fields it names; a `Set` of anything else,
/// an `Add` and a `Remove` write the component whole.
#[test]
fn a_component_write_targets_its_fields_or_the_whole_component() {
    let mut targets = targets_of(&set_health(3, health_patch(&[("max", 1), ("current", 2)])));
    targets.sort_by(|a, b| a.1.cmp(&b.1));
    assert_eq!(
        targets,
        vec![
            (entity(3), health_field("current")),
            (entity(3), health_field("max")),
        ]
    );

    let whole = WriteTarget::Component(ComponentName::new("Health"));
    for command in [
        set_health(3, ScriptValue::Int(5)),
        WorldCommand::AddComponent {
            entity: entity(3),
            component: ComponentName::new("Health"),
            value: health_patch(&[("current", 1)]),
        },
        WorldCommand::RemoveComponent {
            entity: entity(3),
            component: ComponentName::new("Health"),
        },
    ] {
        assert_eq!(
            targets_of(&command),
            vec![(entity(3), whole.clone())],
            "{command:?}"
        );
    }
}

/// The transform and lifecycle commands keep their one target each; a nudge
/// and a spawn have none.
#[test]
fn the_other_commands_keep_their_targets() {
    assert_eq!(
        targets_of(&place(4, 1.0)),
        vec![(entity(4), WriteTarget::Translation)]
    );
    assert_eq!(
        targets_of(&WorldCommand::SetRotation {
            entity: entity(4),
            value: Quaternion::IDENTITY,
        }),
        vec![(entity(4), WriteTarget::Rotation)]
    );
    assert_eq!(
        targets_of(&WorldCommand::SetScale {
            entity: entity(4),
            value: Vec3::ONE,
        }),
        vec![(entity(4), WriteTarget::Scale)]
    );
    assert_eq!(
        targets_of(&WorldCommand::SetParent {
            entity: entity(4),
            parent: None,
        }),
        vec![(entity(4), WriteTarget::Parent)]
    );
    assert_eq!(
        targets_of(&WorldCommand::Despawn { entity: entity(4) }),
        vec![(entity(4), WriteTarget::Existence)]
    );
    assert!(targets_of(&nudge(4, 1.0)).is_empty());
}

/// A field target reads as the component and the field it lands on.
#[test]
fn a_field_target_names_its_component_and_field() {
    assert_eq!(
        health_field("current").to_string(),
        "component Health field current"
    );
    assert_eq!(
        WriteTarget::Component(ComponentName::new("Health")).to_string(),
        "component Health"
    );
}

// ─── Conflicts per field ────────────────────────────────────────────────────

/// **The merge composes them.** Two patches on different fields of one
/// component lose nothing.
#[test]
fn disjoint_fields_do_not_conflict() {
    let mut buffer = CommandBuffer::new();
    buffer.push(set_health(3, health_patch(&[("current", 5)])));
    buffer.push(set_health(3, health_patch(&[("max", 10)])));

    assert!(buffer.conflicts().is_empty(), "{:?}", buffer.conflicts());
}

/// Two patches on the same field lose one write: reported on that field,
/// with the count.
#[test]
fn the_same_field_conflicts() {
    let mut buffer = CommandBuffer::new();
    buffer.push(set_health(3, health_patch(&[("current", 5)])));
    buffer.push(set_health(3, health_patch(&[("current", 6), ("max", 10)])));
    buffer.push(set_health(3, health_patch(&[("current", 7)])));

    let conflicts = buffer.conflicts();
    assert_eq!(conflicts.len(), 1, "{conflicts:?}");
    assert_eq!(conflicts[0].entity, entity(3));
    assert_eq!(conflicts[0].target, health_field("current"));
    assert_eq!(conflicts[0].writes, 3);
    let message = conflicts[0].message();
    assert!(
        message.contains("component Health field current"),
        "got: {message}"
    );
    assert!(message.contains("3 times"), "got: {message}");
}

/// A whole-component write overwrites every field, so it conflicts with any
/// field write to that component of that entity — reported on the component.
/// Disjoint field writes on another entity, beside it, still compose.
#[test]
fn a_whole_component_write_conflicts_with_a_field_write() {
    let whole_writes = [
        WorldCommand::AddComponent {
            entity: entity(3),
            component: ComponentName::new("Health"),
            value: health_patch(&[("current", 1)]),
        },
        WorldCommand::RemoveComponent {
            entity: entity(3),
            component: ComponentName::new("Health"),
        },
        set_health(3, ScriptValue::Int(5)),
    ];

    for whole in whole_writes {
        let mut buffer = CommandBuffer::new();
        buffer.push(set_health(3, health_patch(&[("max", 10)])));
        buffer.push(whole.clone());
        buffer.push(set_health(4, health_patch(&[("current", 1)])));
        buffer.push(set_health(4, health_patch(&[("max", 1)])));

        let conflicts = buffer.conflicts();
        assert_eq!(conflicts.len(), 1, "{whole:?}: {conflicts:?}");
        assert_eq!(conflicts[0].entity, entity(3));
        assert_eq!(
            conflicts[0].target,
            WriteTarget::Component(ComponentName::new("Health")),
            "{whole:?}"
        );
        assert_eq!(conflicts[0].writes, 2, "{whole:?}");
    }
}

/// A cell of a batch and a `Set` of the same field of the same entity lose
/// one write; a `Set` of another field does not.
#[test]
fn a_column_cell_conflicts_with_a_set_of_the_same_field() {
    let mut buffer = CommandBuffer::new();
    buffer.push(set_health(2, health_patch(&[("current", 9)])));
    buffer.push(set_health(1, health_patch(&[("max", 9)])));
    buffer.push(health_batch(
        &["current"],
        &[
            (1, vec![ScriptValue::Int(1)]),
            (2, vec![ScriptValue::Int(2)]),
        ],
    ));

    let conflicts = buffer.conflicts();
    assert_eq!(conflicts.len(), 1, "{conflicts:?}");
    assert_eq!(conflicts[0].entity, entity(2));
    assert_eq!(conflicts[0].target, health_field("current"));
    assert_eq!(conflicts[0].writes, 2);
}

/// **Large batches.** Ten thousand rows, one entity among them twice: one
/// conflict, on that entity's field, and nothing else.
#[test]
fn a_large_batch_reports_its_one_duplicated_cell() {
    let mut batch = ColumnBatch::new(names(&["current"])).expect("one field");
    for index in 0..10_000 {
        batch
            .push_row(entity(index), [ScriptValue::Int(i64::from(index))])
            .expect("fits");
    }
    batch
        .push_row(entity(4_242), [ScriptValue::Int(-1)])
        .expect("fits");
    let mut buffer = CommandBuffer::new();
    buffer.push(WorldCommand::WriteColumn {
        component: ComponentName::new("Health"),
        batch: Arc::new(batch),
    });

    let conflicts = buffer.conflicts();
    assert_eq!(
        conflicts.len(),
        1,
        "{:?}",
        &conflicts[..conflicts.len().min(4)]
    );
    assert_eq!(conflicts[0].entity, entity(4_242));
    assert_eq!(conflicts[0].target, health_field("current"));
    assert_eq!(conflicts[0].writes, 2);
}

/// Field conflicts come back in a fixed order: by entity, then by target,
/// fields of one component by name.
#[test]
fn field_conflicts_are_reported_in_a_stable_order() {
    let mut buffer = CommandBuffer::new();
    buffer.push(set_health(3, health_patch(&[("max", 1), ("current", 1)])));
    buffer.push(set_health(3, health_patch(&[("current", 2), ("max", 2)])));
    buffer.push(place(1, 1.0));
    buffer.push(place(1, 2.0));

    let found: Vec<(u32, WriteTarget)> = buffer
        .conflicts()
        .into_iter()
        .map(|conflict| (conflict.entity.index, conflict.target))
        .collect();
    assert_eq!(
        found,
        vec![
            (1, WriteTarget::Translation),
            (3, health_field("current")),
            (3, health_field("max")),
        ]
    );
}

/// **A whole-component conflict counts writes, not cells.** A removal and
/// one patch of two fields are two writes landing on `Health` — the message
/// says "written 2 times" — as a removal and a two-field batch row are.
#[test]
fn a_whole_component_conflict_counts_each_write_once() {
    let remove = WorldCommand::RemoveComponent {
        entity: entity(3),
        component: ComponentName::new("Health"),
    };
    let field_writes = [
        set_health(3, health_patch(&[("current", 1), ("max", 2)])),
        health_batch(
            &["current", "max"],
            &[(3, vec![ScriptValue::Int(1), ScriptValue::Int(2)])],
        ),
    ];

    for write in field_writes {
        let mut buffer = CommandBuffer::new();
        buffer.push(write.clone());
        buffer.push(remove.clone());

        let conflicts = buffer.conflicts();
        assert_eq!(conflicts.len(), 1, "{write:?}: {conflicts:?}");
        assert_eq!(
            conflicts[0].target,
            WriteTarget::Component(ComponentName::new("Health"))
        );
        assert_eq!(
            conflicts[0].writes,
            2,
            "{write:?}: {}",
            conflicts[0].message()
        );
    }
}

/// **Mixed writes, counted one per command and row.** On entity 3: an add,
/// a two-field patch, a batch holding it twice and a one-field patch are
/// five writes, reported once on the component. On entity 4: a patch and a
/// batch row share only `max`, reported on that field alone. A `Set` of
/// nothing and a batch of no field write nothing and count for nothing.
#[test]
fn mixed_writes_are_counted_one_per_command_and_row() {
    let mut buffer = CommandBuffer::new();
    buffer.push(WorldCommand::AddComponent {
        entity: entity(3),
        component: ComponentName::new("Health"),
        value: ScriptValue::Unit,
    });
    buffer.push(set_health(3, health_patch(&[("current", 1), ("max", 2)])));
    buffer.push(set_health(4, health_patch(&[("current", 1), ("max", 2)])));
    buffer.push(health_batch(
        &["current", "max"],
        &[
            (3, vec![ScriptValue::Int(1), ScriptValue::Int(2)]),
            (3, vec![ScriptValue::Int(3), ScriptValue::Int(4)]),
        ],
    ));
    buffer.push(health_batch(&["max"], &[(4, vec![ScriptValue::Int(9)])]));
    buffer.push(set_health(3, health_patch(&[("max", 5)])));
    buffer.push(set_health(5, ScriptValue::Unit));
    buffer.push(health_batch(&[], &[(5, vec![])]));
    buffer.push(WorldCommand::RemoveComponent {
        entity: entity(5),
        component: ComponentName::new("Health"),
    });

    let found: Vec<(u32, WriteTarget, usize)> = buffer
        .conflicts()
        .into_iter()
        .map(|conflict| (conflict.entity.index, conflict.target, conflict.writes))
        .collect();
    assert_eq!(
        found,
        vec![
            (3, WriteTarget::Component(ComponentName::new("Health")), 5),
            (4, health_field("max"), 2),
        ]
    );
}
