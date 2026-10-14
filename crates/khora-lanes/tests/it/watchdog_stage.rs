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

//! A stage for the watchdog's tests: one module of behaviors, a cast of
//! entities running them, and frames of a chosen game time and fuel — with
//! every body the frames reported as slow kept, in order.

use khora_core::ecs::entity::EntityId;
use khora_core::script::{EventQueue, ScriptEvent, ScriptSnapshot};
use khora_data::flow::{ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script_lane::{run_behaviors, Instance, ScriptRunReport, ScriptRuntime, Slow};
use khora_script::arena::Persisted;
use khora_script::vm::{Program, Value};
use khora_script::{check, compile, ergon_fn, lex, parse, Host};

/// A native costing more than the slices the watchdog's tests hand out, so
/// a frame of that fuel runs one call of it and stops at the next safepoint.
#[ergon_fn(name = "WatchdogWall", cost = 50)]
fn watchdog_wall() -> f32 {
    0.0
}

/// The module every stage's behaviors are compiled as.
pub const MODULE: &str = "watchdog.erg";

/// Fuel enough for a frame to run a call of `WatchdogWall` and be cut.
pub const SLICE: u64 = 20;

/// `source` lexed, parsed, checked and compiled, with no diagnostics.
pub fn build(source: &str) -> Program {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let checked = check(&parsed.module);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
    let compiled = compile(&parsed.module);
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );
    compiled.program
}

/// The entity numbered `index`.
pub fn entity(index: u32) -> EntityId {
    EntityId {
        index,
        generation: 1,
    }
}

/// Event `name`, with no payload, raised at `to`.
pub fn event(to: EntityId, name: &str) -> EventQueue {
    let mut events = EventQueue::new();
    events.push(ScriptEvent::new(to, name));
    events
}

/// A module's behaviors run by a cast of entities, frame after frame.
pub struct Stage {
    pub runtime: ScriptRuntime,
    host: Host,
    cast: Vec<(EntityId, String)>,
    /// What the next frame hands over as the first entity's arrival.
    arriving: Option<ScriptSnapshot>,
    /// Every slow body the frames reported, in order.
    pub slow: Vec<Slow>,
    /// Game seconds the frames have covered.
    pub elapsed: f32,
    /// The frames run.
    pub frames: usize,
}

impl Stage {
    /// `source` run by one entity per `(entity, behavior)` of `cast`, in that
    /// order.
    pub fn new(source: &str, cast: &[(EntityId, &str)]) -> Self {
        let mut runtime = ScriptRuntime::new();
        runtime.add_program(MODULE, build(source));
        Self {
            runtime,
            host: Host::new(),
            cast: cast
                .iter()
                .map(|(entity, behavior)| (*entity, (*behavior).to_owned()))
                .collect(),
            arriving: None,
            slow: Vec::new(),
            elapsed: 0.0,
            frames: 0,
        }
    }

    /// `source` run by `behavior` on entity 0, which arrives as `saved`
    /// records it — what loading a save hands the lane.
    pub fn loaded(source: &str, behavior: &str, saved: ScriptSnapshot) -> Self {
        let mut stage = Self::new(source, &[(entity(0), behavior)]);
        stage.arriving = Some(saved);
        stage
    }

    fn view(&mut self, delta: f32) -> ScriptView {
        let mut programs: Vec<ScriptProgram> = Vec::new();
        let mut instances = Vec::new();
        for (at, (entity, behavior)) in self.cast.iter().enumerate() {
            let program = match programs.iter().position(|p| &p.behavior == behavior) {
                Some(index) => index,
                None => {
                    programs.push(ScriptProgram {
                        module: MODULE.to_owned(),
                        behavior: behavior.clone(),
                    });
                    programs.len() - 1
                }
            };
            let arrival = if at == 0 {
                self.arriving.take().map(crate::saves::loaded)
            } else {
                None
            };
            instances.push(ScriptInstance {
                entity: *entity,
                program: program as u32,
                arrival,
                translation: khora_core::math::Vec3::ZERO,
                rotation: khora_core::math::Quaternion::IDENTITY,
                scale: khora_core::math::Vec3::ONE,
                row: Vec::new(),
            });
        }
        ScriptView {
            resumed: false,
            delta_seconds: delta,
            input: Default::default(),
            components: Default::default(),
            programs,
            instances,
        }
    }

    /// One frame of `delta` game seconds and `fuel`, delivering `events`.
    pub fn frame_with(&mut self, delta: f32, fuel: u64, events: &EventQueue) -> ScriptRunReport {
        let view = self.view(delta);
        let report = run_behaviors(&view, events, &mut self.runtime, &mut self.host, fuel);
        self.slow.extend(report.slow.iter().cloned());
        self.elapsed += delta;
        self.frames += 1;
        report
    }

    /// One frame of `delta` game seconds and `fuel`.
    pub fn frame(&mut self, delta: f32, fuel: u64) -> ScriptRunReport {
        self.frame_with(delta, fuel, &EventQueue::new())
    }

    /// `count` frames of `delta` game seconds and `fuel`; the last one's
    /// report.
    pub fn frames(&mut self, count: usize, delta: f32, fuel: u64) -> ScriptRunReport {
        let mut last = ScriptRunReport::default();
        for _ in 0..count {
            last = self.frame(delta, fuel);
        }
        last
    }

    /// The instance `behavior` on `entity`.
    pub fn instance(&self, entity: EntityId, behavior: &str) -> &Instance {
        self.runtime
            .peek(entity, behavior)
            .unwrap_or_else(|| panic!("`{behavior}` has an instance on {entity:?}"))
    }

    /// Whether `behavior` on `entity` is disabled.
    pub fn disabled(&self, entity: EntityId, behavior: &str) -> bool {
        self.runtime
            .peek(entity, behavior)
            .is_some_and(|instance| instance.disabled)
    }

    /// The game seconds the body `behavior` on `entity` has part-way has
    /// run, `None` with no body part-way.
    pub fn running(&self, entity: EntityId, behavior: &str) -> Option<f32> {
        self.instance(entity, behavior)
            .pending
            .as_ref()
            .map(|pending| pending.running)
    }

    /// How many times `behavior` on `entity` has been reported slow.
    pub fn warnings(&self, entity: EntityId, behavior: &str) -> usize {
        self.slow
            .iter()
            .filter(|slow| slow.entity == entity && slow.behavior == behavior)
            .count()
    }

    /// Field `name` of `behavior` on `entity`, as an `int`.
    pub fn int(&self, entity: EntityId, behavior: &str, name: &str) -> Option<i64> {
        let slot = self
            .runtime
            .program(MODULE)
            .and_then(|program| program.layout(behavior))
            .and_then(|layout| layout.slot_of(name))
            .unwrap_or_else(|| panic!("`{behavior}` has a field `{name}`"));
        match self.runtime.peek(entity, behavior)?.fields.get(slot)? {
            Persisted::Scalar(Value::Int(value)) => Some(*value),
            _ => None,
        }
    }

    /// Runs frames of `delta` and `fuel` until `behavior` on `entity` is
    /// disabled, at most `limit` of them; how many it took, `None` when it
    /// never was.
    pub fn frames_until_disabled(
        &mut self,
        entity: EntityId,
        behavior: &str,
        delta: f32,
        fuel: u64,
        limit: usize,
    ) -> Option<usize> {
        for frame in 1..=limit {
            self.frame(delta, fuel);
            if self.disabled(entity, behavior) {
                return Some(frame);
            }
        }
        None
    }
}

/// The snapshot the lane recorded of `behavior` on `entity` in `report`.
pub fn recorded(report: &ScriptRunReport, entity: EntityId, behavior: &str) -> ScriptSnapshot {
    report
        .state
        .iter()
        .find(|update| update.entity == entity && update.behavior == behavior)
        .map(|update| update.snapshot.clone())
        .expect("the instance did work, so the lane wrote its state")
}
