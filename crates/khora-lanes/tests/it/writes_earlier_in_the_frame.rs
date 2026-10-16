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

//! A turn reads what was written to its fields earlier in the frame.
//!
//! Behaviors take their turns in a fixed order. When one of them `Set`s the
//! component of a behavior whose turn comes later the same frame, that turn
//! starts from the written value — `speed += 1` adds to what was set, not to
//! what the instance held before. A `Set` queued after the turn is not read:
//! it is applied over the turn at the frame boundary. Which of the two a `Set`
//! was is told by the update's `emitted_after`: how many of the frame's
//! commands were queued before the turn's writes took effect, counted in the
//! deck's command buffer, after whatever it already held.

use std::sync::Arc;

use khora_core::ecs::entity::EntityId;
use khora_core::event::{Channel, WhenFull};
use khora_core::lane::{Lane, LaneContext, OutputDeck};
use khora_core::math::Vec3;
use khora_core::script::{
    BehaviorUpdate, BehaviorWriteback, CommandBuffer, ScriptCatalog, ScriptEvent, ScriptValue,
    WorldCommand,
};
use khora_data::flow::{ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script_lane::{BudgetedScriptLane, Fuel, ScriptRuntime};
use khora_script::arena::Persisted;
use khora_script::reload::ScriptReload;
use khora_script::vm::{Program, Value};
use khora_script::{compile_module, Diagnostic, MemoryLoader};

const MODULE: &str = "ai/guard.erg";

/// A pusher that sets a guard's `speed` when told to; a guard that speeds up
/// by one and steps forward every frame.
const PUSH_AND_RUN: &str = "behavior Pusher {
    on Push(Entity target) { target.Set(Guard { speed: 5 }); }
}
behavior Guard {
    float speed = 0;
    void Update(float dt) {
        speed += 1;
        this.Translate(Vec3(0.0, 1.0, 0.0));
    }
}
event Push(Entity target);";

fn program(source: &str) -> Program {
    let result = compile_module(&MemoryLoader::new().with(MODULE, source), MODULE);
    let errors: Vec<&Diagnostic> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "`{MODULE}` should compile: {errors:?}");
    result.program.expect("a program with no errors")
}

fn entity(index: u32) -> EntityId {
    EntityId {
        index,
        generation: 1,
    }
}

const PUSHER: EntityId = EntityId {
    index: 1,
    generation: 1,
};
const RUNNER: EntityId = EntityId {
    index: 2,
    generation: 1,
};

/// A view in which the behaviors take their turns in the order of
/// `behaviors`, each running on its entities.
fn view(behaviors: &[(&str, &[EntityId])]) -> ScriptView {
    let programs = behaviors
        .iter()
        .map(|(behavior, _)| ScriptProgram {
            module: MODULE.to_owned(),
            behavior: (*behavior).to_owned(),
        })
        .collect();
    let instances = behaviors
        .iter()
        .enumerate()
        .flat_map(|(program, (_, entities))| {
            entities.iter().map(move |&entity| ScriptInstance {
                entity,
                program: program as u32,
                arrival: None,
                translation: Vec3::ZERO,
                rotation: khora_core::math::Quaternion::IDENTITY,
                scale: Vec3::ONE,
                row: Vec::new(),
            })
        })
        .collect();
    ScriptView {
        fixed_steps: 0,
        fixed_delta_seconds: 0.0,
        resumed: false,
        delta_seconds: 0.016,
        input: Default::default(),
        components: Default::default(),
        programs,
        instances,
    }
}

/// The lane with what the engine hands it.
struct Stage {
    runtime: ScriptRuntime,
    reloads: Channel<ScriptReload>,
    events: Channel<ScriptEvent>,
    catalog: Arc<ScriptCatalog>,
}

/// What one frame handed over: the commands in the deck, in order, and the
/// instance states for the writeback.
struct Handed {
    commands: Vec<WorldCommand>,
    updates: Vec<BehaviorUpdate>,
}

impl Handed {
    /// The update the lane handed over for `entity`'s `Guard`.
    fn guard(&self, entity: EntityId) -> &BehaviorUpdate {
        self.updates
            .iter()
            .find(|update| update.entity == entity && update.behavior == "Guard")
            .unwrap_or_else(|| panic!("an update for {entity:?}'s `Guard`: {:?}", self.updates))
    }

    /// Where the pusher's `Set` of `target`'s `Guard` sits among the
    /// commands.
    fn set_of(&self, target: EntityId) -> usize {
        self.commands
            .iter()
            .position(|command| {
                matches!(
                    command,
                    WorldCommand::SetComponent { entity, component, .. }
                        if *entity == target && component.as_str() == "Guard"
                )
            })
            .unwrap_or_else(|| panic!("a `Set` of {target:?}'s `Guard`: {:?}", self.commands))
    }
}

impl Stage {
    fn new() -> Self {
        let mut runtime = ScriptRuntime::new();
        runtime.add_program(MODULE, program(PUSH_AND_RUN));
        Self {
            runtime,
            reloads: Channel::bounded(64, WhenFull::DropOldest),
            events: Channel::bounded(64, WhenFull::DropOldest),
            catalog: Arc::new(ScriptCatalog::default()),
        }
    }

    /// Tells the pusher to push `target` this frame.
    fn push(&self, target: EntityId) {
        self.events
            .send(ScriptEvent::new(PUSHER, "Push").with(ScriptValue::Entity(target)));
    }

    /// One frame through the lane, its deck's command buffer already holding
    /// `before` — what another lane queued earlier in the tick.
    fn frame(&mut self, view: &ScriptView, before: &[WorldCommand]) -> Handed {
        let lane = BudgetedScriptLane::new();
        let mut deck = OutputDeck::new();
        deck.slot::<CommandBuffer>().extend(before.iter().cloned());
        {
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
        Handed {
            commands: deck.slot::<CommandBuffer>().as_slice().to_vec(),
            updates: deck.slot::<BehaviorWriteback>().drain().collect(),
        }
    }

    /// `entity`'s `Guard.speed`, as the lane holds it.
    fn speed(&self, entity: EntityId) -> Option<f32> {
        let slot = self
            .runtime
            .program(MODULE)?
            .layout("Guard")?
            .slot_of("speed")?;
        match self.runtime.peek(entity, "Guard")?.fields.get(slot)? {
            Persisted::Scalar(Value::Float(value)) => Some(*value),
            _ => None,
        }
    }
}

/// `speed` as an update recorded it.
fn recorded_speed(update: &BehaviorUpdate) -> Option<f32> {
    match update
        .snapshot
        .fields
        .iter()
        .find(|(name, _)| name == "speed")?
        .1
    {
        ScriptValue::Float(value) => Some(value),
        _ => None,
    }
}

/// Commands another lane queued before the scripts ran.
fn queued_elsewhere(count: u32) -> Vec<WorldCommand> {
    (0..count)
        .map(|index| WorldCommand::SetTranslation {
            entity: entity(100 + index),
            value: Vec3::ZERO,
        })
        .collect()
}

/// **A turn starts from a `Set` queued earlier in the frame.** The pusher's
/// turn comes first and sets the guard's `speed` to 5; the guard's `speed += 1`
/// then makes 6 — not 1, which is what it would make of the value it held.
#[test]
fn a_turn_starts_from_a_set_of_its_field_queued_earlier_in_the_frame() {
    let mut stage = Stage::new();
    let view = view(&[("Pusher", &[PUSHER]), ("Guard", &[RUNNER])]);

    stage.push(RUNNER);
    let handed = stage.frame(&view, &[]);

    assert_eq!(stage.speed(RUNNER), Some(6.0), "5 set, then one more");
    let update = handed.guard(RUNNER);
    assert_eq!(recorded_speed(update), Some(6.0));
    assert!(
        update.changed.iter().any(|field| field == "speed"),
        "the turn changed `speed`: {:?}",
        update.changed
    );
}

/// What is set on one entity's behavior is read by that entity's turn only:
/// another guard starts from what it held.
#[test]
fn a_set_of_another_entitys_behavior_is_not_read() {
    let mut stage = Stage::new();
    let pushed = entity(3);
    let view = view(&[("Pusher", &[PUSHER]), ("Guard", &[RUNNER, pushed])]);

    stage.push(pushed);
    stage.frame(&view, &[]);

    assert_eq!(stage.speed(pushed), Some(6.0));
    assert_eq!(stage.speed(RUNNER), Some(1.0), "not pushed, it ran from 0");
}

/// A turn that read the `Set` reads it once: the next frame adds to what the
/// turn made, not to the `Set` again.
#[test]
fn a_set_read_by_a_turn_is_not_read_again_the_next_frame() {
    let mut stage = Stage::new();
    let view = view(&[("Pusher", &[PUSHER]), ("Guard", &[RUNNER])]);

    stage.push(RUNNER);
    stage.frame(&view, &[]);
    stage.frame(&view, &[]);

    assert_eq!(stage.speed(RUNNER), Some(7.0), "5, then +1 twice");
}

/// **The update says where the turn sits among the frame's commands**: every
/// command queued before its writes took effect — the pusher's `Set`, and the
/// guard's own step forward — so the `Set` is one the turn read.
#[test]
fn an_update_counts_the_commands_queued_before_its_turn_took_effect() {
    let mut stage = Stage::new();
    let view = view(&[("Pusher", &[PUSHER]), ("Guard", &[RUNNER])]);

    stage.push(RUNNER);
    let handed = stage.frame(&view, &[]);

    assert_eq!(handed.commands.len(), 2, "{:?}", handed.commands);
    let update = handed.guard(RUNNER);
    assert_eq!(update.emitted_after, 2, "the `Set` and its own step");
    assert!(
        handed.set_of(RUNNER) < update.emitted_after,
        "the `Set` it read sits before it"
    );
}

/// The count is of the deck's command buffer, which the lane extends: what
/// another lane queued earlier in the tick comes first, and the turn's
/// position is counted after it.
#[test]
fn an_update_counts_the_commands_the_deck_already_held() {
    let mut stage = Stage::new();
    let view = view(&[("Pusher", &[PUSHER]), ("Guard", &[RUNNER])]);

    stage.push(RUNNER);
    let handed = stage.frame(&view, &queued_elsewhere(3));

    assert_eq!(handed.commands.len(), 5, "{:?}", handed.commands);
    assert_eq!(handed.set_of(RUNNER), 3, "after what the deck held");
    assert_eq!(
        handed.guard(RUNNER).emitted_after,
        5,
        "three held, the `Set`, its own step"
    );
}

/// **A `Set` queued after the turn is not read.** The guard's turn comes
/// first, so it runs from what it held; the pusher's `Set` sits after the
/// turn among the deck's commands — the one the boundary applies over it.
#[test]
fn a_set_queued_after_the_turn_is_not_read_and_sits_after_it() {
    let mut stage = Stage::new();
    let view = view(&[("Guard", &[RUNNER]), ("Pusher", &[PUSHER])]);

    stage.push(RUNNER);
    let handed = stage.frame(&view, &queued_elsewhere(2));

    assert_eq!(stage.speed(RUNNER), Some(1.0), "it ran from what it held");
    let update = handed.guard(RUNNER);
    assert_eq!(recorded_speed(update), Some(1.0));
    assert_eq!(
        update.emitted_after, 3,
        "two held, then its own step; the `Set` came later"
    );
    assert!(
        handed.set_of(RUNNER) >= update.emitted_after,
        "the `Set` sits after the turn: {} vs {}",
        handed.set_of(RUNNER),
        update.emitted_after
    );
}
