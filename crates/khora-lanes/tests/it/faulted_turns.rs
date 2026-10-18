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

//! **A turn that faults hands over none of its commands.** What it queued
//! before the fault — a spawn, a write, an event — leaves the frame with it:
//! the commands the lane hands the boundary and the events it keeps for the
//! next frame are the other turns' alone, and the working set the later turns
//! read holds none of the faulted turn's writes. The turns before it and
//! after it keep all of theirs.

use std::collections::HashMap;
use std::sync::Arc;

use khora_core::ecs::entity::EntityId;
use khora_core::math::{Quaternion, Vec3};
use khora_core::script::{
    ColumnSnapshot, ComponentName, ComponentProjection, EventQueue, FrameComponents,
    ProjectedComponent, ScriptValue, WorldCommand,
};
use khora_data::flow::{ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script_lane::{run_behaviors, ScriptRuntime};
use khora_script::native::Host;

use super::declared_behaviors::program;

const MODULE: &str = "game/faulty.erg";

/// `Before` and `After` write their own `Health` and raise; `Faulty`, between
/// them, spawns, writes its own `Health` and raises, then divides by zero.
const SOURCE: &str = "component Health { int current = 0; int max = 0; }
event Hurt;
behavior Before {
    void Update(float dt) { this.Set(Health { current: 2 }); this.Raise(Hurt()); }
}
behavior Faulty {
    int zero = 0;
    void Update(float dt) {
        Spawn(Vec3(0.0, 0.0, 0.0), Health { current: 1, max: 1 });
        this.Set(Health { current: 9 });
        this.Raise(Hurt());
        zero = 1 / zero;
    }
}
behavior After {
    void Update(float dt) { this.Set(Health { current: 4 }); this.Raise(Hurt()); }
}";

const BEFORE: EntityId = EntityId {
    index: 1,
    generation: 1,
};
const FAULTY: EntityId = EntityId {
    index: 2,
    generation: 1,
};
const AFTER: EntityId = EntityId {
    index: 3,
    generation: 1,
};

/// A column of rows already converted.
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
    ScriptValue::Struct(vec![
        ("current".to_owned(), ScriptValue::Int(current)),
        ("max".to_owned(), ScriptValue::Int(max)),
    ])
}

/// `Health` read, each of the three holding 50 of 50.
fn projection() -> ComponentProjection {
    let holders = [BEFORE, FAULTY, AFTER];
    let index: HashMap<EntityId, (u32, u32)> = holders
        .iter()
        .enumerate()
        .map(|(row, entity)| (*entity, (0, row as u32)))
        .collect();
    ComponentProjection::new(vec![ProjectedComponent::new(
        ComponentName::new("Health"),
        vec![Box::new(Rows(vec![health(50, 50); holders.len()]))],
        index,
        Some(health(0, 0)),
    )])
}

/// The three behaviors, each on its entity, taking their turns in the order
/// `Before`, `Faulty`, `After`.
fn view() -> ScriptView {
    let behaviors = [("Before", BEFORE), ("Faulty", FAULTY), ("After", AFTER)];
    ScriptView {
        fixed_steps: 0,
        fixed_delta_seconds: 0.0,
        resumed: false,
        delta_seconds: 0.016,
        input: Default::default(),
        components: Arc::new(projection()),
        programs: behaviors
            .iter()
            .map(|(behavior, _)| ScriptProgram {
                module: MODULE.to_owned(),
                behavior: (*behavior).to_owned(),
            })
            .collect(),
        instances: behaviors
            .iter()
            .enumerate()
            .map(|(program, (_, entity))| ScriptInstance {
                entity: *entity,
                program: program as u32,
                arrival: None,
                translation: Vec3::ZERO,
                rotation: Quaternion::IDENTITY,
                scale: Vec3::ONE,
                row: Vec::new(),
            })
            .collect(),
    }
}

/// One frame of the three: the host it ran on, after it.
fn frame() -> Host {
    let mut runtime = ScriptRuntime::new();
    runtime.add_program(MODULE, program(MODULE, SOURCE));
    let view = view();
    let mut host = Host::new();
    host.components = FrameComponents::new(Arc::clone(&view.components));

    let report = run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);

    assert_eq!(report.faulted, 1, "the faulty turn faulted: {report:?}");
    assert_eq!(report.completed, 2, "the others ran: {report:?}");
    host
}

fn set_of(entity: EntityId, current: i64) -> WorldCommand {
    WorldCommand::SetComponent {
        entity,
        component: ComponentName::new("Health"),
        value: ScriptValue::Struct(vec![("current".to_owned(), ScriptValue::Int(current))]),
    }
}

/// **The commands handed over are the other turns'**, in their order: no
/// spawn, no write of the faulted entity.
#[test]
fn a_faulted_turn_hands_over_none_of_its_commands() {
    let host = frame();

    assert_eq!(
        host.commands.as_slice(),
        &[set_of(BEFORE, 2), set_of(AFTER, 4)],
        "the faulted turn's spawn and write were handed over"
    );
}

/// **The events kept for the next frame are the other turns'**: the faulted
/// turn's is dropped.
#[test]
fn a_faulted_turn_raises_nothing() {
    let mut host = frame();

    let raised: Vec<EntityId> = host
        .take_events()
        .as_slice()
        .iter()
        .map(|event| event.target)
        .collect();
    assert_eq!(raised, vec![BEFORE, AFTER]);
}

/// **The working set holds none of the faulted turn's writes**: what the
/// turns after it read of the faulted entity is what it held.
#[test]
fn the_working_set_forgets_a_faulted_turns_writes() {
    let host = frame();

    assert_eq!(host.components.get("Health", FAULTY), Some(health(50, 50)));
    assert_eq!(host.components.get("Health", AFTER), Some(health(4, 50)));
    assert_eq!(host.components.get("Health", BEFORE), Some(health(2, 50)));
}
