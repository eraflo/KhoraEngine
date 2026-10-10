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
use crate::script::{ComponentName, ScriptValue, WorldCommand};

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
