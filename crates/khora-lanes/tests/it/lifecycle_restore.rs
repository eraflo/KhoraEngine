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

//! Where a recorded lifecycle and `OnLoad` give way.
//!
//! An instance restored from a save runs `OnLoad` once and never `OnSpawn`
//! again; one that starts fresh runs `OnSpawn` once; and the body a save
//! caught part-way finishes once.
//!
//! The behaviors are components: an entity holding `Guard` runs `Guard`, and
//! what the engine keeps of the instance is its state companion
//! `Guard#state` — what a game save restores.

use std::sync::{Arc, RwLock};

use khora_core::agent::{EngineMode, SharedEngineMode};
use khora_core::ecs::entity::EntityId;
use khora_core::script::{EventQueue, InstanceLifecycle, ScriptEvent, ScriptSnapshot, ScriptValue};
use khora_core::Runtime;
use khora_data::ecs::{Transform, World};
use khora_data::flow::{Flow, ScriptFlow, ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script_lane::{run_behaviors, ScriptRuntime};
use khora_script::arena::Persisted;
use khora_script::native::Host;
use khora_script::vm::Value;

use super::declared_behaviors::{attach, declared, program, recorded, restore, write_back};

const MODULE: &str = "lifecycle_restore.erg";

/// A script runtime holding `source`'s program, for a hand-built view.
fn runtime_of(source: &str) -> ScriptRuntime {
    let mut runtime = ScriptRuntime::new();
    runtime.add_program(MODULE, program(MODULE, source));
    runtime
}

fn subject() -> EntityId {
    EntityId {
        index: 0,
        generation: 1,
    }
}

/// A hand-built view of the one subject, with no arrival.
fn view_of(behavior: &str, delta: f32) -> ScriptView {
    ScriptView {
        fixed_steps: 0,
        fixed_delta_seconds: 0.0,
        delta_seconds: delta,
        resumed: false,
        input: Default::default(),
        components: Default::default(),
        programs: vec![ScriptProgram {
            module: MODULE.to_owned(),
            behavior: behavior.to_owned(),
        }],
        instances: vec![ScriptInstance {
            entity: subject(),
            program: 0,
            arrival: None,
            translation: khora_core::math::Vec3::ZERO,
            rotation: khora_core::math::Quaternion::IDENTITY,
            scale: khora_core::math::Vec3::ONE,
            row: Vec::new(),
        }],
    }
}

fn names(host: &mut Host) -> Vec<String> {
    host.take_events()
        .as_slice()
        .iter()
        .map(|event| event.name.clone())
        .collect()
}

fn count(raised: &[String], name: &str) -> usize {
    raised.iter().filter(|known| *known == name).count()
}

fn int_of(runtime: &ScriptRuntime, entity: EntityId, behavior: &str, name: &str) -> Option<i64> {
    let slot = runtime.program(MODULE)?.layout(behavior)?.slot_of(name)?;
    match runtime.peek(entity, behavior)?.fields.get(slot)? {
        Persisted::Scalar(Value::Int(value)) => Some(*value),
        _ => None,
    }
}

/// `saved` as a state that says `OnSpawn` never ran: what a save written
/// before the lifecycle was recorded reads as.
fn never_spawned(saved: ScriptSnapshot) -> ScriptSnapshot {
    let old = ScriptSnapshot {
        lifecycle: InstanceLifecycle::default(),
        ..saved
    };
    assert!(!old.lifecycle.spawned, "it reads as never spawned");
    old
}

/// A world projected through the real flow under the frame's mode.
struct Game {
    world: World,
    flow: ScriptFlow,
    runtime: Runtime,
    mode: SharedEngineMode,
}

impl Game {
    fn new(world: World) -> Self {
        let mode: SharedEngineMode = Arc::new(RwLock::new(EngineMode::Playing));
        let mut runtime = Runtime::default();
        runtime.resources.insert(Arc::clone(&mode));
        Self {
            world,
            flow: ScriptFlow::default(),
            runtime,
            mode,
        }
    }

    fn view(&mut self) -> ScriptView {
        let selection = self.flow.select(&self.world, &self.runtime);
        self.flow.project(&self.world, &selection, &self.runtime)
    }

    fn set_mode(&self, mode: EngineMode) {
        *self.mode.write().expect("the mode lock") = mode;
    }
}

// ─── An arrival the frame could not afford ─────────────────────────────────

/// Restored or fresh, it does not matter: what the lane must not do is the
/// opposite of both.
const LOADER: &str = r#"behavior Loader {
                           int health = 100;
                           int greeted = 0;
                           int loaded = 0;
                           void OnSpawn() { greeted += 1; this.Raise(Greeted()); }
                           void OnLoad() { loaded += 1; this.Raise(Loaded()); }
                       }
                       event Greeted;
                       event Loaded;"#;

/// **An arrival is handed over once, and a frame out of fuel drops it.** The
/// flow delivers what an entity brings only on the first frame it appears;
/// the lane skips an instance the frame can no longer afford before reading
/// that arrival. Under budget pressure — a save of many scripted entities,
/// loaded on a frame whose fuel the earlier ones spend — the later ones are
/// exactly those instances.
///
/// A restored entity whose arrival is dropped then runs as a brand new one:
/// it announces itself again, never runs `OnLoad`, and forgets the health
/// the save held.
#[test]
fn a_restored_entity_deferred_on_its_arrival_frame_still_loads() {
    let saved = ScriptSnapshot {
        lifecycle: khora_core::script::InstanceLifecycle {
            spawned: true,
            fault: None,
            resume_failed: Vec::new(),
            owed: 0.0,
            fixed_owed: Vec::new(),
        },
        ..ScriptSnapshot::default()
    }
    .with_field("health", ScriptValue::Int(40))
    .with_field("greeted", ScriptValue::Int(1));

    // Two restored guards; the first is listed first and spends the frame.
    let (mut world, mut runtime) = declared(MODULE, LOADER);
    let first = world.spawn(Transform::identity());
    restore(&mut world, first, "Loader", &saved);
    let second = world.spawn(Transform::identity());
    restore(&mut world, second, "Loader", &saved);
    let mut game = Game::new(world);
    let mut host = Host::new();

    // The frame they arrive on, with fuel for one instruction.
    let view = game.view();
    assert!(
        view.instances
            .iter()
            .all(|instance| instance.arrival.is_some()),
        "both arrive now"
    );
    assert_eq!(view.instances[0].entity, first, "the first is listed first");
    let report = run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, 1);
    assert_eq!(report.deferred, 2, "the budget bites: {report:?}");
    host.take_events();

    let mut raised: Vec<(EntityId, String)> = Vec::new();
    for _ in 0..3 {
        let view = game.view();
        run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, u64::MAX);
        raised.extend(
            host.take_events()
                .as_slice()
                .iter()
                .map(|event| (event.target, event.name.clone())),
        );
    }
    let of = |entity: EntityId, name: &str| {
        raised
            .iter()
            .filter(|(target, known)| *target == entity && known == name)
            .count()
    };

    assert_eq!(of(first, "Loaded"), 1, "the first loaded: {raised:?}");
    assert_eq!(
        of(second, "Greeted"),
        0,
        "a restored entity that had spawned does not announce itself again: {raised:?}"
    );
    assert_eq!(of(second, "Loaded"), 1, "it runs OnLoad: {raised:?}");
    assert_eq!(
        int_of(&runtime, second, "Loader", "health"),
        Some(40),
        "with the health the save held"
    );
}

// ─── OnSpawn owed beside a restored body ────────────────────────────────────

/// Its `OnSpawn` waits, and a spotted guard winds up an attack.
const WAVER: &str = r#"behavior Guard {
                          int fired = 0;
                          async void Attack() {
                              await 1.0s;
                              fired += 1;
                              this.Raise(Fired());
                          }
                          async void Wave() {
                              await 0.5s;
                              this.Raise(Waved());
                          }
                          on Spotted(int by) { Attack(); }
                          void OnSpawn() { Wave(); }
                      }
                      event Spotted(int by);
                      event Fired;
                      event Waved;"#;

/// **A restored state that says `OnSpawn` never ran.** The state companion
/// holds the attack the save caught part-way, and says the instance has not
/// spawned — as a save written before the lifecycle was recorded reads. The
/// load owes `OnSpawn` *and* the attack: `OnSpawn` runs, waits, and is
/// stored as the body part-way — over the attack, which is never resumed.
#[test]
fn an_on_spawn_that_waits_does_not_drop_the_body_a_save_restored() {
    // The running game: OnSpawn waves and finishes, then the guard is spotted
    // and its attack is caught part-way.
    let mut running = runtime_of(WAVER);
    let mut host = Host::new();
    run_behaviors(
        &view_of("Guard", 0.0),
        &EventQueue::new(),
        &mut running,
        &mut host,
        u64::MAX,
    );
    let mut spotted = EventQueue::new();
    spotted.push(ScriptEvent::new(subject(), "Spotted").with(ScriptValue::Int(1)));
    let report = run_behaviors(
        &view_of("Guard", 0.6),
        &spotted,
        &mut running,
        &mut host,
        u64::MAX,
    );
    assert_eq!(count(&names(&mut host), "Waved"), 1, "OnSpawn finished");
    let saved = report
        .state
        .first()
        .map(|update| update.snapshot.clone())
        .expect("the lane wrote the guard");
    assert!(saved.pending.is_some(), "the attack is part-way: {saved:?}");

    // As a save from before the lifecycle was recorded holds it.
    let old = never_spawned(saved);

    let (mut world, mut loaded) = declared(MODULE, WAVER);
    let guard = world.spawn(Transform::identity());
    restore(&mut world, guard, "Guard", &old);
    let mut game = Game::new(world);
    let mut host = Host::new();
    let mut raised = Vec::new();
    for _ in 0..6 {
        let mut view = game.view();
        view.delta_seconds = 0.6;
        run_behaviors(&view, &EventQueue::new(), &mut loaded, &mut host, u64::MAX);
        raised.extend(names(&mut host));
    }

    assert_eq!(
        count(&raised, "Fired"),
        1,
        "the attack the save caught part-way finishes once: {raised:?}"
    );
}

// ─── A mode that is not the game, mid-game ──────────────────────────────────

const ANNOUNCER: &str = r#"behavior Guard {
                              int steps = 0;
                              void OnSpawn() { this.Raise(Spawned()); }
                              void OnLoad() { this.Raise(Loaded()); }
                              void Update(float dt) { steps += 1; }
                          }
                          event Loaded;
                          event Spawned;"#;

/// **Leaving the game for a moment is not loading a save.** A game that
/// switches to a mode of its own — a photo mode, a cutscene tool — and back
/// keeps its world: nothing was restored. But the flow forgets what it had
/// seen, so every entity arrives again carrying the state companion the
/// writeback recorded, and the lane takes that for a save: `OnLoad` runs for
/// every instance.
#[test]
fn returning_to_play_from_another_mode_is_not_a_load() {
    let (mut world, mut runtime) = declared(MODULE, ANNOUNCER);
    let guard = world.spawn(Transform::identity());
    attach(&mut world, guard, "Guard", &[]);
    let mut game = Game::new(world);
    let mut host = Host::new();
    let mut raised = Vec::new();

    for _ in 0..2 {
        let view = game.view();
        let report = run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, 1_000);
        write_back(&mut game.world, &report);
        raised.extend(names(&mut host));
    }
    assert_eq!(count(&raised, "Spawned"), 1, "it announced itself");
    assert!(
        recorded(&game.world, guard, "Guard").is_some(),
        "the writeback recorded its state companion"
    );

    // A frame in another mode: the scripting agent is registered for
    // `Playing` only, so the lane does not run; the flow still selects.
    game.set_mode(EngineMode::Custom("photo".to_owned()));
    assert!(game.view().is_empty(), "nothing projected");

    game.set_mode(EngineMode::Playing);
    for _ in 0..2 {
        let view = game.view();
        let report = run_behaviors(&view, &EventQueue::new(), &mut runtime, &mut host, 1_000);
        write_back(&mut game.world, &report);
        raised.extend(names(&mut host));
    }

    assert_eq!(count(&raised, "Spawned"), 1, "still once: {raised:?}");
    assert_eq!(
        count(&raised, "Loaded"),
        0,
        "no save was loaded, so OnLoad does not run: {raised:?}"
    );
}

// ─── The first frame back, with nothing scripted ────────────────────────────

/// Says goodbye where anything can hear it.
const LEAVER: &str = r#"behavior Guard {
                           void OnDespawn() { this.Raise(Bye()); }
                       }
                       event Bye;"#;

/// One frame through the lane itself, the way the scripting agent runs it.
fn lane_frame(view: &ScriptView, runtime: &mut ScriptRuntime) {
    use khora_core::lane::{Lane, LaneContext, OutputDeck};
    use khora_lanes::script_lane::{BudgetedScriptLane, Fuel};

    let lane = BudgetedScriptLane::new();
    let mut deck = OutputDeck::new();
    let mut ctx = LaneContext::new();
    ctx.insert_ref(view);
    ctx.insert(Fuel(u64::MAX));
    ctx.insert_slot(runtime);
    ctx.insert_slot(&mut deck);
    lane.execute(&mut ctx).expect("the lane runs");
}

/// **What left while play was away is forgotten — even when nothing is
/// scripted on the first frame back.** The forgetting happens inside
/// `run_behaviors`, on the one frame the view says it resumes; but the lane
/// returns before `run_behaviors` when the view is empty. The instance of an
/// entity that left while play was away then survives that frame, and the
/// first later frame with anything scripted bids it the farewell the
/// resumption was meant to withhold.
#[test]
fn an_entity_that_left_while_play_was_away_gets_no_farewell_later() {
    let (mut world, mut runtime) = declared(MODULE, LEAVER);
    let gone = world.spawn(Transform::identity());
    attach(&mut world, gone, "Guard", &[]);
    let mut game = Game::new(world);

    let view = game.view();
    lane_frame(&view, &mut runtime);
    assert!(runtime.peek(gone, "Guard").is_some(), "it runs");

    // Away from play: the entity leaves, and nothing scripted is left.
    game.set_mode(EngineMode::Custom("photo".to_owned()));
    assert!(game.view().is_empty());
    game.world.despawn(gone);

    // Back: the first frame says it resumes, and has nothing to run.
    game.set_mode(EngineMode::Playing);
    let back = game.view();
    assert!(back.resumed, "the first frame back resumes");
    assert!(back.is_empty(), "nothing scripted");
    lane_frame(&back, &mut runtime);

    // Later, something scripted appears.
    let newcomer = game.world.spawn(Transform::identity());
    attach(&mut game.world, newcomer, "Guard", &[]);
    let later = game.view();
    assert!(!later.resumed);
    lane_frame(&later, &mut runtime);

    let raised = runtime.take_pending();
    let farewells = raised
        .as_slice()
        .iter()
        .filter(|event| event.name == "Bye")
        .count();
    assert_eq!(
        farewells, 0,
        "an entity that left while play was away is not bid farewell"
    );
}

// ─── OnSpawn cut behind a restored body, then saved ─────────────────────────

/// **A cut `OnSpawn` must still finish across a save.** A restored state
/// that says `OnSpawn` never ran owes it and a body part-way. `OnSpawn` runs,
/// waits, and the restored body waits behind it. A save taken now records
/// the restored body — and `spawned`. Loaded, `OnSpawn` is not owed and its
/// rest is not pending: it never finishes, in any run of the game.
#[test]
fn an_on_spawn_cut_behind_a_restored_body_finishes_across_a_save() {
    // The running game, as in the test above: OnSpawn has waved, the attack
    // is part-way.
    let mut running = runtime_of(WAVER);
    let mut host = Host::new();
    run_behaviors(
        &view_of("Guard", 0.0),
        &EventQueue::new(),
        &mut running,
        &mut host,
        u64::MAX,
    );
    let mut spotted = EventQueue::new();
    spotted.push(ScriptEvent::new(subject(), "Spotted").with(ScriptValue::Int(1)));
    let report = run_behaviors(
        &view_of("Guard", 0.6),
        &spotted,
        &mut running,
        &mut host,
        u64::MAX,
    );
    host.take_events();
    let saved = report
        .state
        .first()
        .map(|update| update.snapshot.clone())
        .expect("the lane wrote the guard");
    let old = never_spawned(saved);

    // Loaded: OnSpawn runs and waits; a save is taken that frame.
    let (mut world, mut loaded) = declared(MODULE, WAVER);
    let guard = world.spawn(Transform::identity());
    restore(&mut world, guard, "Guard", &old);
    let mut game = Game::new(world);
    let mut host = Host::new();
    let view = game.view();
    let report = run_behaviors(&view, &EventQueue::new(), &mut loaded, &mut host, u64::MAX);
    let mut waved = count(&names(&mut host), "Waved");
    write_back(&mut game.world, &report);
    let resaved = recorded(&game.world, guard, "Guard").expect("the writeback recorded the guard");

    // Loaded again: whatever the save holds, OnSpawn's wave must come.
    let (mut world, mut again) = declared(MODULE, WAVER);
    let guard = world.spawn(Transform::identity());
    restore(&mut world, guard, "Guard", &resaved);
    let mut game = Game::new(world);
    let mut host = Host::new();
    for _ in 0..6 {
        let mut view = game.view();
        view.delta_seconds = 0.6;
        run_behaviors(&view, &EventQueue::new(), &mut again, &mut host, u64::MAX);
        waved += count(&names(&mut host), "Waved");
    }

    assert_eq!(
        waved, 1,
        "the OnSpawn the first load started finishes once, across the save: {resaved:?}"
    );
}
