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

//! The projection reads a holder's row and nobody else's; the working set
//! answers what this frame's commands made of it.

use std::collections::HashMap;
use std::sync::Arc;

use super::{ColumnSnapshot, ComponentProjection, FrameComponents, ProjectedComponent};
use crate::ecs::entity::EntityId;
use crate::math::Vec3;
use crate::script::{ColumnBatch, ComponentName, ScriptValue, WorldCommand};

fn entity(index: u32) -> EntityId {
    EntityId {
        index,
        generation: 1,
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

/// The value of field `name` of a struct value.
fn field(value: &ScriptValue, name: &str) -> Option<ScriptValue> {
    value
        .as_fields()?
        .iter()
        .find(|(field, _)| field == name)
        .map(|(_, value)| value.clone())
}

/// A column of rows already converted, for tests.
struct Rows(Vec<ScriptValue>);

impl ColumnSnapshot for Rows {
    fn len(&self) -> usize {
        self.0.len()
    }
    fn read(&self, row: usize) -> ScriptValue {
        self.0[row].clone()
    }
}

fn health(current: i64, max: i64) -> ScriptValue {
    fields(&[
        ("current", ScriptValue::Int(current)),
        ("max", ScriptValue::Int(max)),
    ])
}

/// `Health`, held by entities 1 (in the first column) and 3 (in the second);
/// the second column also holds a row nobody's index points at — an orphan.
fn health_projection() -> ComponentProjection {
    let first = Rows(vec![health(5, 10)]);
    let second = Rows(vec![health(99, 99), health(7, 20)]);
    let index = HashMap::from([(entity(1), (0, 0)), (entity(3), (1, 1))]);
    ComponentProjection::new(vec![ProjectedComponent::new(
        ComponentName::new("Health"),
        vec![Box::new(first), Box::new(second)],
        index,
        Some(health(100, 100)),
    )])
}

fn working_set() -> FrameComponents {
    FrameComponents::new(Arc::new(health_projection()))
}

fn set(target: EntityId, patch: ScriptValue) -> WorldCommand {
    WorldCommand::SetComponent {
        entity: target,
        component: ComponentName::new("Health"),
        value: patch,
    }
}

fn add(target: EntityId, patch: ScriptValue) -> WorldCommand {
    WorldCommand::AddComponent {
        entity: target,
        component: ComponentName::new("Health"),
        value: patch,
    }
}

fn remove(target: EntityId) -> WorldCommand {
    WorldCommand::RemoveComponent {
        entity: target,
        component: ComponentName::new("Health"),
    }
}

// ─── The projection ─────────────────────────────────────────────────────────

/// **A holder's row, from whichever column holds it.** An entity the index
/// does not name holds nothing, whatever rows its columns carry.
#[test]
fn a_projected_component_reads_its_holders_rows() {
    let projection = health_projection();
    let health_of = projection.component("Health").expect("projected");

    assert_eq!(health_of.get(entity(1)), Some(health(5, 10)));
    assert_eq!(health_of.get(entity(3)), Some(health(7, 20)));
    assert_eq!(health_of.get(entity(2)), None, "not a holder");
    assert!(health_of.has(entity(1)));
    assert!(!health_of.has(entity(2)));
    assert_eq!(health_of.holders(), 2, "the orphan row is not a holder");
    assert_eq!(health_of.default_row(), Some(&health(100, 100)));
}

/// What was projected is named; what was not, is not there.
#[test]
fn a_projection_names_what_it_projected() {
    let projection = health_projection();

    assert_eq!(projection.names(), vec![&ComponentName::new("Health")]);
    assert!(projection.component("Collider").is_none());
    assert_eq!(projection.rows(), 2, "the rows indexed");
}

/// An empty projection — nothing reads — answers nothing, and costs nothing.
#[test]
fn an_empty_projection_holds_nothing() {
    let projection = ComponentProjection::default();

    assert!(projection.names().is_empty());
    assert_eq!(projection.rows(), 0);
    assert!(projection.component("Health").is_none());
}

// ─── The working set ────────────────────────────────────────────────────────

/// Before any command, the working set is the projection.
#[test]
fn the_working_set_starts_as_the_projection() {
    let frame = working_set();

    assert_eq!(frame.get("Health", entity(1)), Some(health(5, 10)));
    assert!(frame.has("Health", entity(3)));
    assert!(!frame.has("Health", entity(2)));
    assert_eq!(frame.get("Health", entity(2)), None);
}

/// **A `Set` merges the fields it names** into the current copy; the others
/// keep the projected value, and a second `Set` merges over the first.
#[test]
fn a_set_merges_into_the_copy() {
    let mut frame = working_set();

    frame.observe(&set(entity(1), fields(&[("current", ScriptValue::Int(8))])));
    assert_eq!(frame.get("Health", entity(1)), Some(health(8, 10)));

    frame.observe(&set(entity(1), fields(&[("max", ScriptValue::Int(12))])));
    assert_eq!(frame.get("Health", entity(1)), Some(health(8, 12)));
    assert_eq!(
        frame.get("Health", entity(3)),
        Some(health(7, 20)),
        "another holder is untouched"
    );
}

/// A `Set` on an entity that holds nothing attaches nothing: the applier
/// would find no row to write.
#[test]
fn a_set_on_an_entity_without_the_component_changes_nothing() {
    let mut frame = working_set();

    frame.observe(&set(entity(2), fields(&[("current", ScriptValue::Int(8))])));

    assert!(!frame.has("Health", entity(2)));
    assert_eq!(frame.get("Health", entity(2)), None);
}

/// **An `Add` starts from the default row**, then the fields it names.
#[test]
fn an_add_starts_from_the_default_row() {
    let mut frame = working_set();

    frame.observe(&add(
        entity(2),
        fields(&[("current", ScriptValue::Int(40))]),
    ));

    assert!(frame.has("Health", entity(2)));
    assert_eq!(frame.get("Health", entity(2)), Some(health(40, 100)));
}

/// An `Add` to a holder is refused by the applier, so the working set keeps
/// the copy it had rather than resetting it to the defaults.
#[test]
fn an_add_to_a_holder_changes_nothing() {
    let mut frame = working_set();

    frame.observe(&add(
        entity(1),
        fields(&[("current", ScriptValue::Int(40))]),
    ));

    assert_eq!(frame.get("Health", entity(1)), Some(health(5, 10)));
}

/// **A `Remove` hides the component** for the rest of the frame — and an
/// `Add` after it attaches a fresh one, from the defaults.
#[test]
fn a_remove_hides_the_component_until_it_is_added_again() {
    let mut frame = working_set();

    frame.observe(&remove(entity(1)));
    assert!(!frame.has("Health", entity(1)));
    assert_eq!(frame.get("Health", entity(1)), None);

    frame.observe(&add(entity(1), fields(&[("max", ScriptValue::Int(30))])));
    assert!(frame.has("Health", entity(1)));
    assert_eq!(frame.get("Health", entity(1)), Some(health(100, 30)));
}

/// Commands about a component nobody reads, and placement commands, leave
/// the working set as it was.
#[test]
fn commands_about_other_things_leave_the_working_set_alone() {
    let mut frame = working_set();

    frame.observe(&WorldCommand::AddComponent {
        entity: entity(1),
        component: ComponentName::new("Collider"),
        value: fields(&[("friction", ScriptValue::Float(0.8))]),
    });
    frame.observe(&WorldCommand::SetTranslation {
        entity: entity(1),
        value: Vec3::new(1.0, 0.0, 0.0),
    });
    frame.observe(&WorldCommand::Spawn {
        position: Vec3::ZERO,
        rotation: crate::math::Quaternion::IDENTITY,
        components: vec![(ComponentName::new("Health"), health(1, 1))],
    });

    assert!(!frame.has("Collider", entity(1)), "Collider is not read");
    assert_eq!(frame.get("Health", entity(1)), Some(health(5, 10)));
    assert_eq!(
        field(&frame.get("Health", entity(3)).expect("held"), "current"),
        Some(ScriptValue::Int(7))
    );
}

// ─── Column writes ──────────────────────────────────────────────────────────

/// A `Health` column write of `fields`, one row per `(entity, values)`.
fn write_column(fields: &[&str], rows: &[(EntityId, Vec<ScriptValue>)]) -> WorldCommand {
    let mut batch = ColumnBatch::new(fields.iter().map(|f| (*f).to_owned()).collect())
        .expect("distinct fields");
    for (target, values) in rows {
        batch
            .push_row(*target, values.clone())
            .expect("a row of the batch's width");
    }
    WorldCommand::WriteColumn {
        component: ComponentName::new("Health"),
        batch: Arc::new(batch),
    }
}

/// **A batch merges each row** into the copy its entity holds, as a `Set`
/// of the same fields would; a row of an entity holding nothing attaches
/// nothing.
#[test]
fn a_column_write_merges_each_row_into_the_copy() {
    let mut frame = working_set();

    frame.observe(&write_column(
        &["current"],
        &[
            (entity(1), vec![ScriptValue::Int(8)]),
            (entity(3), vec![ScriptValue::Int(9)]),
            (entity(2), vec![ScriptValue::Int(4)]),
        ],
    ));

    assert_eq!(frame.get("Health", entity(1)), Some(health(8, 10)));
    assert_eq!(frame.get("Health", entity(3)), Some(health(9, 20)));
    assert!(!frame.has("Health", entity(2)), "not a holder");
    assert_eq!(frame.get("Health", entity(2)), None);
}

/// Folded in emission order: a `Set` after the batch wins over it, a batch
/// after a `Set` wins over that, and of two rows of one entity the later.
#[test]
fn a_column_write_is_folded_in_emission_order() {
    let mut frame = working_set();

    frame.observe(&set(entity(1), fields(&[("current", ScriptValue::Int(2))])));
    frame.observe(&write_column(
        &["current", "max"],
        &[
            (entity(1), vec![ScriptValue::Int(3), ScriptValue::Int(30)]),
            (entity(1), vec![ScriptValue::Int(4), ScriptValue::Int(40)]),
        ],
    ));
    assert_eq!(frame.get("Health", entity(1)), Some(health(4, 40)));

    frame.observe(&set(entity(1), fields(&[("max", ScriptValue::Int(41))])));
    assert_eq!(frame.get("Health", entity(1)), Some(health(4, 41)));
}

/// A batch of a component nobody reads leaves the working set alone.
#[test]
fn a_column_write_of_an_untracked_component_changes_nothing() {
    let mut frame = working_set();
    let mut batch = ColumnBatch::new(vec!["friction".to_owned()]).expect("one field");
    batch
        .push_row(entity(1), [ScriptValue::Float(0.8)])
        .expect("fits");

    frame.observe(&WorldCommand::WriteColumn {
        component: ComponentName::new("Collider"),
        batch: Arc::new(batch),
    });

    assert!(!frame.has("Collider", entity(1)));
    assert_eq!(frame.get("Health", entity(1)), Some(health(5, 10)));
}

/// A batch follows what the frame did to its entities before it: a row of
/// an entity whose component was removed attaches nothing back, a row of
/// one that was added this frame lands on the added copy.
#[test]
fn a_column_write_follows_the_removals_and_adds_before_it() {
    let mut frame = working_set();
    frame.observe(&remove(entity(1)));
    frame.observe(&add(entity(2), fields(&[("max", ScriptValue::Int(50))])));

    frame.observe(&write_column(
        &["current"],
        &[
            (entity(1), vec![ScriptValue::Int(8)]),
            (entity(2), vec![ScriptValue::Int(9)]),
        ],
    ));

    assert!(
        !frame.has("Health", entity(1)),
        "a removed component came back"
    );
    assert_eq!(frame.get("Health", entity(1)), None);
    assert_eq!(frame.get("Health", entity(2)), Some(health(9, 50)));
}

/// `Health` held by entities 9, 2 and 5; `Dead` by 5.
fn census() -> FrameComponents {
    let holders = [
        (entity(9), health(9, 9)),
        (entity(2), health(2, 2)),
        (entity(5), health(5, 5)),
    ];
    let health_column = ProjectedComponent::new(
        ComponentName::new("Health"),
        vec![Box::new(Rows(
            holders.iter().map(|(_, row)| row.clone()).collect(),
        ))],
        holders
            .iter()
            .enumerate()
            .map(|(row, (entity, _))| (*entity, (0, row as u32)))
            .collect(),
        Some(health(100, 100)),
    );
    let dead = ProjectedComponent::new(
        ComponentName::new("Dead"),
        vec![Box::new(Rows(vec![fields(&[])]))],
        HashMap::from([(entity(5), (0, 0))]),
        Some(fields(&[])),
    );
    FrameComponents::new(Arc::new(ComponentProjection::new(vec![
        health_column,
        dead,
    ])))
}

#[test]
fn matching_lists_the_holders_in_entity_order() {
    let frame = census();
    assert_eq!(
        frame.matching(&["Health"], &[]),
        [entity(2), entity(5), entity(9)]
    );
    assert_eq!(
        frame.matching(&["Health"], &["Dead"]),
        [entity(2), entity(9)]
    );
    assert_eq!(frame.matching(&["Health", "Dead"], &[]), [entity(5)]);
    assert_eq!(
        frame.scan_cost(&["Health", "Dead"]),
        1,
        "the fewer holders drive"
    );
}

#[test]
fn matching_follows_this_frames_adds_and_removes() {
    let mut frame = census();
    frame.observe(&WorldCommand::AddComponent {
        entity: entity(4),
        component: ComponentName::new("Health"),
        value: fields(&[]),
    });
    frame.observe(&WorldCommand::RemoveComponent {
        entity: entity(9),
        component: ComponentName::new("Health"),
    });
    frame.observe(&WorldCommand::RemoveComponent {
        entity: entity(5),
        component: ComponentName::new("Dead"),
    });
    assert_eq!(
        frame.matching(&["Health"], &["Dead"]),
        [entity(2), entity(4), entity(5)]
    );
}

#[test]
fn matching_nothing_required_or_unprojected_is_empty() {
    let frame = census();
    assert!(frame.matching(&[], &["Dead"]).is_empty());
    assert!(frame.matching(&["Health", "Ghost"], &[]).is_empty());
    assert_eq!(frame.scan_cost(&["Ghost"]), 0);
}

/// **Each entity is one row, however the frame got it there**: a holder
/// taken out and put back, an entity given the component then rid of it, a
/// recycled index holding it under a new generation — each candidate is
/// listed, and counted by `scan_cost`, once.
#[test]
fn matching_lists_each_entity_once_across_removes_and_adds() {
    let mut frame = census();
    frame.observe(&remove(entity(2)));
    frame.observe(&add(entity(2), fields(&[])));
    frame.observe(&add(entity(4), fields(&[])));
    frame.observe(&remove(entity(4)));
    assert_eq!(
        frame.matching(&["Health"], &[]),
        [entity(2), entity(5), entity(9)]
    );
    assert_eq!(frame.scan_cost(&["Health"]), 3);

    let recycled = EntityId {
        index: 2,
        generation: 2,
    };
    frame.observe(&add(recycled, fields(&[])));
    assert_eq!(
        frame.matching(&["Health"], &[]),
        [entity(2), recycled, entity(5), entity(9)],
        "in entity order, generation after index"
    );
    assert_eq!(frame.scan_cost(&["Health"]), 4);
}
