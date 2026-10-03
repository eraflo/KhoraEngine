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

//! Bodies that resume where they stopped, attacked across modules and across a
//! save taken while a body waits.

use khora_core::ecs::entity::EntityId;
use khora_core::script::{EventQueue, ScriptEvent, ScriptSnapshot, ScriptValue};
use khora_data::flow::{ScriptArrival, ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script_lane::{run_behaviors, ScriptRunReport, ScriptRuntime};
use khora_script::arena::Persisted;
use khora_script::vm::{Program, Value};
use khora_script::{check, compile, lex, parse, Host};

use super::saves::every_encoding;

const ARMOURY: &str = "armoury.erg";
const BARRACKS: &str = "barracks.erg";

fn build(source: &str) -> Program {
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

fn subject() -> EntityId {
    EntityId {
        index: 0,
        generation: 1,
    }
}

fn view_of(
    module: &str,
    behavior: &str,
    delta: f32,
    authored: Option<ScriptSnapshot>,
) -> ScriptView {
    ScriptView {
        delta_seconds: delta,
        input: Default::default(),
        programs: vec![ScriptProgram {
            module: module.to_owned(),
            behavior: behavior.to_owned(),
        }],
        instances: vec![ScriptInstance {
            entity: subject(),
            program: 0,
            // What the lane wrote back arrives as observed state.
            arrival: authored.map(|observed| ScriptArrival {
                fields: Vec::new(),
                observed: Some(observed),
            }),
            translation: khora_core::math::Vec3::ZERO,
            rotation: khora_core::math::Quaternion::IDENTITY,
            scale: khora_core::math::Vec3::ONE,
        }],
    }
}

fn int_at(runtime: &ScriptRuntime, behavior: &str, slot: usize) -> Option<i64> {
    match runtime.peek(subject(), behavior)?.fields.get(slot)? {
        Persisted::Scalar(Value::Int(value)) => Some(*value),
        _ => None,
    }
}

fn spotted() -> EventQueue {
    let mut events = EventQueue::new();
    events.push(ScriptEvent::new(subject(), "Spotted").with(ScriptValue::Int(1)));
    events
}

fn quiet(runtime: &mut ScriptRuntime, host: &mut Host, view: &ScriptView) -> ScriptRunReport {
    run_behaviors(view, &EventQueue::new(), runtime, host, u64::MAX)
}

/// Waits a second inside an attack, then lands it.
const ATTACKER: &str = "behavior Guard {
                            int fired = 0;
                            async void Attack() {
                                await 1.0s;
                                fired += 1;
                            }
                            on Spotted(int by) { Attack(); }
                        }";

/// Another module's `Guard`: same field, different code.
const OTHER_GUARD: &str = "behavior Guard {
                               int fired = 0;
                               on Spotted(int by) { fired += 10; }
                           }";

/// [`OTHER_GUARD`] edited — a method added, so a different program.
const OTHER_GUARD_EDITED: &str = "behavior Guard {
                                      int fired = 0;
                                      void Idle() { }
                                      on Spotted(int by) { fired += 10; }
                                  }";

// ─── A reload of one module reaches into another ────────────────────────────

/// **Editing one module leaves another module's instances alone.** Two modules
/// each declare a `Guard`; an entity running the armoury's is waiting inside an
/// attack when the author edits the barracks' `Guard`. The reload walks every
/// instance *named* `Guard` whatever module it runs, sees a pending machine
/// whose fingerprint is not the barracks' new one, and abandons it — the
/// armoury guard, whose code nobody touched, forgets its attack.
#[test]
fn a_reload_of_another_module_does_not_abandon_this_modules_sequence() {
    let mut runtime = ScriptRuntime::new();
    runtime.add_program(ARMOURY, build(ATTACKER));
    runtime.add_program(BARRACKS, build(OTHER_GUARD));
    let mut host = Host::new();

    run_behaviors(
        &view_of(ARMOURY, "Guard", 0.0, None),
        &spotted(),
        &mut runtime,
        &mut host,
        u64::MAX,
    );
    assert!(
        runtime
            .peek(subject(), "Guard")
            .is_some_and(|instance| instance.pending.is_some()),
        "the armoury guard is waiting inside its attack"
    );

    runtime.reload(BARRACKS, build(OTHER_GUARD_EDITED));
    for _ in 0..3 {
        quiet(
            &mut runtime,
            &mut host,
            &view_of(ARMOURY, "Guard", 0.5, None),
        );
    }

    assert_eq!(
        int_at(&runtime, "Guard", 0),
        Some(1),
        "the armoury guard's attack was abandoned by an edit to the barracks"
    );
}

// ─── A save taken while a body waits ────────────────────────────────────────

/// **A save taken while a body waits records how long it still has to wait.**
/// A guard spotted at the start waits two seconds inside its attack. One and a
/// half seconds later the game is saved: the running game lands the attack
/// half a second after that. The wait's countdown moves without costing fuel,
/// and a behavior with no schedule writes its state only when it spends fuel —
/// so the scene still holds the snapshot from the frame the wait began, two
/// full seconds. The loaded guard waits them all over again.
#[test]
fn a_save_taken_during_a_wait_records_what_is_left_of_it() {
    const SLOW_ATTACKER: &str = "behavior Guard {
                                     int fired = 0;
                                     async void Attack() {
                                         await 2.0s;
                                         fired += 1;
                                     }
                                     on Spotted(int by) { Attack(); }
                                 }";

    let mut running = ScriptRuntime::new();
    running.add_program(ARMOURY, build(SLOW_ATTACKER));
    let mut host = Host::new();

    // What the scene holds is the last state the lane sent for the instance.
    let mut scene: Option<ScriptSnapshot> = None;
    let mut keep = |report: &ScriptRunReport| {
        if let Some(update) = report.state.first() {
            scene = Some(update.snapshot.clone());
        }
    };

    keep(&run_behaviors(
        &view_of(ARMOURY, "Guard", 0.0, None),
        &spotted(),
        &mut running,
        &mut host,
        u64::MAX,
    ));
    for _ in 0..3 {
        keep(&quiet(
            &mut running,
            &mut host,
            &view_of(ARMOURY, "Guard", 0.5, None),
        ));
    }
    let saved = scene.clone().expect("the lane sent the guard's state");

    // Half a second more lands the attack in the running game.
    quiet(
        &mut running,
        &mut host,
        &view_of(ARMOURY, "Guard", 0.5, None),
    );
    assert_eq!(int_at(&running, "Guard", 0), Some(1), "the running game");

    // The same half second after loading the save.
    let mut loaded = ScriptRuntime::new();
    loaded.add_program(ARMOURY, build(SLOW_ATTACKER));
    let mut host = Host::new();
    quiet(
        &mut loaded,
        &mut host,
        &view_of(ARMOURY, "Guard", 0.0, Some(saved.clone())),
    );
    quiet(
        &mut loaded,
        &mut host,
        &view_of(ARMOURY, "Guard", 0.5, None),
    );

    assert_eq!(
        int_at(&loaded, "Guard", 0),
        Some(1),
        "the save recorded the wait as it began, not as it stood: {:?}",
        saved.pending.as_ref().map(|pending| pending.remaining)
    );
}

// ─── A machine the engine writes and cannot read back ───────────────────────

/// **A save the engine writes, the engine reads back.** A behavior's `Update`
/// walks sixty calls deep carrying a long line of text — a literal of the
/// script, about a hundred kilobytes — and the frame's budget runs out at the
/// bottom. The lane keeps the machine and the scene records it. A structured
/// machine writes each literal register as its full text, so sixty frames
/// holding the one literal come to several megabytes. Every encoding a scene
/// carries the snapshot in reads it back whole: one that refused it would drop
/// the entity's `Script`, so the entity would load with no behavior.
#[test]
fn a_save_holding_a_deep_machine_reads_back() {
    let line = "x".repeat(100_000);
    let source = format!(
        "behavior Narrator {{
             int depth = 0;
             void Down(int n, string text) {{
                 if (n > 0) {{
                     Down(n - 1, text);
                     return;
                 }}
                 int spin = 0;
                 for (int i = 0; i < 100000; i = i + 1) {{ spin = spin + 1; }}
             }}
             void Update(float dt) {{ Down(60, \"{line}\"); }}
         }}"
    );

    let mut runtime = ScriptRuntime::new();
    runtime.add_program(ARMOURY, build(&source));
    let mut host = Host::new();
    let cut = run_behaviors(
        &view_of(ARMOURY, "Narrator", 0.1, None),
        &EventQueue::new(),
        &mut runtime,
        &mut host,
        5_000,
    );
    let snapshot = cut
        .state
        .first()
        .map(|update| update.snapshot.clone())
        .expect("the narrator did work, so the lane wrote its state");
    assert!(
        snapshot.pending.is_some(),
        "the cut `Update` is kept as the instance's pending body"
    );

    for (encoding, carry) in every_encoding() {
        let loaded: ScriptSnapshot = carry(&snapshot);
        assert!(
            loaded == snapshot,
            "{encoding}: the snapshot the engine wrote does not read back whole"
        );
    }
}
