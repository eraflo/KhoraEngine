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

//! **Stop, then Play: no mail of the old session reaches the new one.**
//!
//! An editor's Stop restores the scene; its next Play is a play session
//! starting again — the lane sees it as a view that is `resumed`. What was
//! queued for the old world — what scripts raised in its last frame, and what
//! the engine put on its channel while nothing played — belongs to entities
//! that may no longer exist, or whose ids now name someone else. None of it
//! is delivered after Play. What the new session raises is, as ever, the
//! next frame.

use std::sync::Arc;

use khora_core::ecs::entity::EntityId;
use khora_core::event::{Channel, WhenFull};
use khora_core::lane::{Lane, LaneContext, OutputDeck};
use khora_core::script::{ScriptCatalog, ScriptEvent, ScriptValue};
use khora_data::flow::{ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script_lane::{BudgetedScriptLane, Fuel, ScriptRuntime};
use khora_script::arena::Persisted;
use khora_script::reload::ScriptReload;
use khora_script::vm::{Program, Value};
use khora_script::{compile_module, Diagnostic, MemoryLoader};

const MODULE: &str = "ai/guard.erg";

/// A guard that hits itself once, from its first `Update`.
const SELF_HITTER: &str = "event Damaged(int amount);

behavior Guard {
    int health = 100;
    int hits = 0;
    bool armed = true;

    void Update(float dt) {
        if (armed) {
            armed = false;
            this.Raise(Damaged(25));
        }
    }

    on Damaged(int amount) {
        health -= amount;
        hits += 1;
    }
}";

/// A guard that only hears.
const HEARING: &str = "event Damaged(int amount);

behavior Guard {
    int health = 100;
    int hits = 0;

    on Damaged(int amount) {
        health -= amount;
        hits += 1;
    }
}";

fn program(source: &str) -> Program {
    let result = compile_module(&MemoryLoader::new().with(MODULE, source), MODULE);
    let errors: Vec<&Diagnostic> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "should compile: {errors:?}");
    result.program.expect("a program with no errors")
}

/// The id a guard has in both sessions: the restored scene handed the same
/// index and generation to the entity that now holds it.
fn guard() -> EntityId {
    EntityId {
        index: 0,
        generation: 1,
    }
}

/// A frame's view of `entities` running `Guard`; `resumed` on the first frame
/// of a play session.
fn view(entities: &[EntityId], resumed: bool) -> ScriptView {
    ScriptView {
        resumed,
        delta_seconds: 0.016,
        input: Default::default(),
        components: Default::default(),
        programs: vec![ScriptProgram {
            module: MODULE.to_owned(),
            behavior: "Guard".to_owned(),
        }],
        instances: entities
            .iter()
            .map(|&entity| ScriptInstance {
                entity,
                program: 0,
                arrival: None,
                translation: khora_core::math::Vec3::ZERO,
                rotation: khora_core::math::Quaternion::IDENTITY,
                scale: khora_core::math::Vec3::ONE,
                row: Vec::new(),
            })
            .collect(),
    }
}

/// The lane with what the engine hands it: its channels and the catalog.
struct Stage {
    runtime: ScriptRuntime,
    reloads: Channel<ScriptReload>,
    events: Channel<ScriptEvent>,
    catalog: Arc<ScriptCatalog>,
}

impl Stage {
    fn new(source: &str) -> Self {
        let mut runtime = ScriptRuntime::new();
        runtime.add_program(MODULE, program(source));
        Self {
            runtime,
            reloads: Channel::bounded(64, WhenFull::DropOldest),
            events: Channel::bounded(64, WhenFull::DropOldest),
            catalog: Arc::new(ScriptCatalog::default()),
        }
    }

    /// One frame through the lane.
    fn frame(&mut self, view: &ScriptView) {
        let lane = BudgetedScriptLane::new();
        let mut deck = OutputDeck::new();
        let mut ctx = LaneContext::new();
        ctx.insert_ref(view);
        ctx.insert(Fuel(u64::MAX));
        ctx.insert_slot(&mut self.runtime);
        ctx.insert_slot(&mut deck);
        ctx.insert_ref(&self.reloads);
        ctx.insert_ref(&self.events);
        ctx.insert_ref(&self.catalog);
        lane.execute(&mut ctx).expect("the lane runs");
    }

    /// `Damaged(amount)` at `target`, as the engine's channel carries it.
    fn engine_hits(&self, target: EntityId, amount: i64) {
        self.events
            .send(ScriptEvent::new(target, "Damaged").with(ScriptValue::Int(amount)));
    }

    /// Field `name` of `entity`'s `Guard`.
    fn int(&self, entity: EntityId, name: &str) -> Option<i64> {
        let slot = self
            .runtime
            .program(MODULE)?
            .layout("Guard")?
            .slot_of(name)?;
        match self.runtime.peek(entity, "Guard")?.fields.get(slot)? {
            Persisted::Scalar(Value::Int(value)) => Some(*value),
            _ => None,
        }
    }
}

/// **What a script raised in the last frame before Stop is not delivered
/// after Play** — to the entity that holds its target's id now.
#[test]
fn mail_a_script_raised_before_stop_does_not_reach_an_entity_after_play() {
    let mut stage = Stage::new(SELF_HITTER);
    stage.frame(&view(&[guard()], false));
    assert_eq!(
        stage.runtime.pending_len(),
        1,
        "the premise: the hit is queued for the next frame"
    );

    // Stop, then Play: the first frame of the new session.
    stage.frame(&view(&[guard()], true));

    assert_eq!(
        stage.int(guard(), "hits"),
        Some(0),
        "the old session's hit reached the new session's guard"
    );
}

/// **The engine channel's backlog of the old session is not delivered after
/// Play** — what was put on it after the last played frame was for the old
/// world.
#[test]
fn the_engine_backlog_from_before_stop_does_not_reach_an_entity_after_play() {
    let mut stage = Stage::new(HEARING);
    stage.frame(&view(&[guard()], false));
    stage.engine_hits(guard(), 10);

    // Stop, then Play.
    stage.frame(&view(&[guard()], true));

    assert_eq!(
        stage.int(guard(), "hits"),
        Some(0),
        "the old session's engine event reached the new session's guard"
    );
    assert_eq!(stage.int(guard(), "health"), Some(100));
}

/// **Nor across a Play on an empty scene**: mail held over a session that
/// started with nothing in it does not reach an entity that appears later
/// under the old id.
#[test]
fn mail_held_over_an_empty_play_does_not_reach_a_later_entity() {
    let mut stage = Stage::new(SELF_HITTER);
    stage.frame(&view(&[guard()], false));
    stage.engine_hits(guard(), 10);
    assert_eq!(stage.runtime.pending_len(), 1, "the premise: a hit queued");

    // Stop, then Play on a scene with no guard; then one spawns.
    stage.frame(&view(&[], true));
    stage.frame(&view(&[guard()], false));

    assert_eq!(
        stage.int(guard(), "hits"),
        Some(0),
        "mail of the old session reached a guard of the new one"
    );
}

/// **What the new session raises is delivered as ever** — the restart clears
/// the old mail, not the first frame's own: a script's raise in the first
/// frame of Play, and what the engine sends after it, reach the guard the
/// next frame.
#[test]
fn mail_of_the_first_frame_of_play_reaches_the_next() {
    let mut stage = Stage::new(SELF_HITTER);

    stage.frame(&view(&[guard()], true));
    stage.engine_hits(guard(), 10);
    stage.frame(&view(&[guard()], false));

    assert_eq!(stage.int(guard(), "hits"), Some(2), "both landed");
    assert_eq!(stage.int(guard(), "health"), Some(65));
}
