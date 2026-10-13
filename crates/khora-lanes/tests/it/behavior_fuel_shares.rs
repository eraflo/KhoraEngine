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

//! **A frame's fuel, shared among behaviors.** Each behavior is handed a fair
//! share of what the frame has left, in the view's program order: fuel a
//! behavior cannot spend is not lost to the others, no behavior is left out
//! of every frame by the ones before it, and a behavior's rotation resumes
//! near where it stopped even when the instance it stopped at has gone.

use khora_core::ecs::entity::EntityId;
use khora_core::script::EventQueue;
use khora_data::flow::{ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script_lane::{run_behaviors, ScriptRunReport, ScriptRuntime};
use khora_script::vm::Program;
use khora_script::{check, compile, ergon_fn, lex, parse, Host};

/// Far more fuel than any frame below spends.
const PLENTY: u64 = 10_000_000;

/// A native whose one call costs more than the small frames below hold.
#[ergon_fn(name = "ShareCostly", cost = 200)]
fn share_costly() -> f32 {
    1.0
}

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

fn entity(index: u32) -> EntityId {
    EntityId {
        index,
        generation: 1,
    }
}

/// A view of `programs`, then each instance as `(entity index, program
/// index)`, in the order given.
fn view(programs: &[(&str, &str)], instances: &[(u32, u32)]) -> ScriptView {
    ScriptView {
        resumed: false,
        delta_seconds: 0.016,
        input: Default::default(),
        components: Default::default(),
        programs: programs
            .iter()
            .map(|&(module, behavior)| ScriptProgram {
                module: module.to_owned(),
                behavior: behavior.to_owned(),
            })
            .collect(),
        instances: instances
            .iter()
            .map(|&(index, program)| ScriptInstance {
                entity: entity(index),
                program,
                arrival: None,
                translation: khora_core::math::Vec3::ZERO,
                rotation: khora_core::math::Quaternion::IDENTITY,
                scale: khora_core::math::Vec3::ONE,
                row: None,
            })
            .collect(),
    }
}

fn frame(view: &ScriptView, runtime: &mut ScriptRuntime, fuel: u64) -> ScriptRunReport {
    let mut host = Host::new();
    run_behaviors(view, &EventQueue::new(), runtime, &mut host, fuel)
}

/// The entities, by index, whose `behavior` did work this frame.
fn ran(report: &ScriptRunReport, behavior: &str) -> Vec<u32> {
    report
        .state
        .iter()
        .filter(|update| update.behavior == behavior)
        .map(|update| update.entity.index)
        .collect()
}

const TICK: &str = "behavior Tick { int ticks = 0; void Update(float dt) { ticks += 1; } }";

/// What one `Tick` turn costs once spawned, checked to be exact.
fn tick_turn(view: &ScriptView, runtime: &mut ScriptRuntime, ticks: u64) -> u64 {
    frame(view, runtime, PLENTY);
    let report = frame(view, runtime, PLENTY);
    assert_eq!(report.completed as u64, ticks, "{report:?}");
    assert_eq!(report.spent % ticks, 0, "{report:?}");
    report.spent / ticks
}

/// **A behavior with no program does not withhold fuel from the one before
/// it.** `Tick` (first in the view) has ten instances; `Ghost`, after it, has
/// one and no program loaded, so it can spend nothing. With fuel for six
/// turns, `Tick` should run six a frame — the frame has nothing else to
/// spend them on — not the three of a half share, with the rest of the
/// frame's fuel left unspent and seven instances reported deferred.
#[test]
fn a_behavior_with_no_program_does_not_withhold_fuel_from_the_one_before() {
    let mut runtime = ScriptRuntime::new();
    runtime.add_program("tick.erg", build(TICK));
    let instances: Vec<(u32, u32)> = (0..10).map(|i| (i, 0)).chain([(10, 1)]).collect();
    let view = view(&[("tick.erg", "Tick"), ("ghost.erg", "Ghost")], &instances);
    let per_turn = tick_turn(&view, &mut runtime, 10);

    for at in 0..3 {
        let report = frame(&view, &mut runtime, per_turn * 6);
        assert_eq!(
            ran(&report, "Tick").len(),
            6,
            "frame {at}: fuel for six turns, and only `Tick` can spend it — {report:?}"
        );
    }
}

/// **A behavior whose every instance faulted does not withhold fuel from the
/// one before it.** As above, with `Broken` loaded but disabled by its fault:
/// it will never spend again, and every frame from then on `Tick` is held to
/// half the fuel, the other half left unspent.
#[test]
fn a_faulted_behavior_does_not_withhold_fuel_from_the_one_before() {
    let mut runtime = ScriptRuntime::new();
    runtime.add_program("tick.erg", build(TICK));
    runtime.add_program(
        "broken.erg",
        build("behavior Broken { int zero = 0; void Update(float dt) { int x = 1 / zero; } }"),
    );
    let instances: Vec<(u32, u32)> = (0..10).map(|i| (i, 0)).chain([(10, 1)]).collect();
    let view = view(
        &[("tick.erg", "Tick"), ("broken.erg", "Broken")],
        &instances,
    );
    frame(&view, &mut runtime, PLENTY);
    let report = frame(&view, &mut runtime, PLENTY);
    assert_eq!(report.completed, 10, "{report:?}");
    let per_turn = report.spent / 10;
    assert!(
        runtime
            .peek(entity(10), "Broken")
            .is_some_and(|instance| instance.disabled),
        "`Broken` faulted and is disabled"
    );

    for at in 0..3 {
        let report = frame(&view, &mut runtime, per_turn * 6);
        assert_eq!(
            ran(&report, "Tick").len(),
            6,
            "frame {at}: fuel for six turns, and only `Tick` can spend it — {report:?}"
        );
    }
}

/// **A behavior is not left out of every frame by one that overdrafts before
/// it.** `Hog` (first) calls a native costing 200 at every step, so whatever
/// share it is handed it spends past the whole frame's fuel of 100; `Tick`,
/// after it, gets nothing that frame. That is fine once — not every frame:
/// within ten frames `Tick` must have run at least once.
#[test]
fn a_behavior_after_an_overdrafting_one_still_runs_within_bounded_frames() {
    let mut runtime = ScriptRuntime::new();
    runtime.add_program(
        "hog.erg",
        build(
            "behavior Hog {
                 int calls = 0;
                 void Update(float dt) { while (true) { float c = ShareCostly(); calls += 1; } }
             }",
        ),
    );
    runtime.add_program("tick.erg", build(TICK));
    let view = view(
        &[("hog.erg", "Hog"), ("tick.erg", "Tick")],
        &[(0, 0), (1, 1)],
    );
    // Both spawned and initialised.
    frame(&view, &mut runtime, PLENTY);

    let mut tick_ran = 0;
    for _ in 0..10 {
        let report = frame(&view, &mut runtime, 100);
        tick_ran += ran(&report, "Tick").len();
    }
    assert!(tick_ran > 0, "`Tick` never ran in ten frames of 100 fuel");
}

/// **Fewer units of fuel than behaviors leaves no behavior out of every
/// frame.** Three behaviors, one instance each, and one unit of fuel a frame:
/// the first is handed it every frame and the others never are. Over thirty
/// frames each of the three should have done some work.
#[test]
fn fuel_below_the_number_of_behaviors_reaches_every_behavior() {
    let mut runtime = ScriptRuntime::new();
    for name in ["A", "B", "C"] {
        runtime.add_program(
            format!("{name}.erg"),
            build(&format!(
                "behavior {name} {{ int ticks = 0; void Update(float dt) {{ ticks += 1; }} }}"
            )),
        );
    }
    let view = view(
        &[("A.erg", "A"), ("B.erg", "B"), ("C.erg", "C")],
        &[(0, 0), (1, 1), (2, 2)],
    );
    frame(&view, &mut runtime, PLENTY);

    let mut worked = std::collections::HashSet::new();
    for _ in 0..30 {
        let report = frame(&view, &mut runtime, 1);
        for name in ["A", "B", "C"] {
            if !ran(&report, name).is_empty() {
                worked.insert(name);
            }
        }
    }
    assert_eq!(worked.len(), 3, "only {worked:?} did any work in 30 frames");
}

/// **A behavior's rotation resumes after where it stopped when the instance
/// it stopped at has gone.** Ten `Tick`s, fuel for three: the first frame runs
/// 0 1 2 and stops at 3. Entity 3 is despawned before the next frame, which
/// should carry on from 4 — not start over at 0 and run 0 1 2 again, which,
/// repeated, leaves the tail out for as long as the instance at the cursor
/// keeps going.
#[test]
fn a_rotation_resumes_after_its_cursor_when_the_cursor_s_instance_has_gone() {
    let mut runtime = ScriptRuntime::new();
    runtime.add_program("tick.erg", build(TICK));
    let all: Vec<(u32, u32)> = (0..10).map(|i| (i, 0)).collect();
    let full = view(&[("tick.erg", "Tick")], &all);
    let per_turn = tick_turn(&full, &mut runtime, 10);

    let first = frame(&full, &mut runtime, per_turn * 3);
    let mut first_ran = ran(&first, "Tick");
    first_ran.sort_unstable();
    assert_eq!(first_ran, vec![0, 1, 2], "{first:?}");

    let without_3: Vec<(u32, u32)> = all.iter().copied().filter(|&(i, _)| i != 3).collect();
    let next = view(&[("tick.erg", "Tick")], &without_3);
    let mut second_ran = ran(&frame(&next, &mut runtime, per_turn * 3), "Tick");
    second_ran.sort_unstable();
    assert_eq!(
        second_ran,
        vec![4, 5, 6],
        "the rotation carries on past the instance that left"
    );
}

fn hits(runtime: &ScriptRuntime, index: u32, behavior: &str) -> Option<i64> {
    match runtime.peek(entity(index), behavior)?.fields.get(0) {
        Some(khora_script::arena::Persisted::Scalar(khora_script::vm::Value::Int(n))) => Some(*n),
        _ => None,
    }
}

/// **An event for an entity running two behaviors reaches each once, however
/// the shares fall.** Entity 0 runs `A` and `B`; `B` also runs on 1–5, listed
/// before 0, so with fuel for a few turns `B`'s share runs out before it
/// reaches 0 for several frames. One `Hit` for entity 0, raised once: `A`
/// hears it at once, `B` when its rotation reaches 0 — each exactly once.
#[test]
fn an_event_reaches_each_behavior_of_its_entity_once_under_shares() {
    let mut runtime = ScriptRuntime::new();
    for name in ["A", "B"] {
        runtime.add_program(
            format!("{name}.erg"),
            build(&format!(
                "behavior {name} {{ int hits = 0; int ticks = 0;
                     void Update(float dt) {{ ticks += 1; }}
                     on Hit() {{ hits += 1; }} }}"
            )),
        );
    }
    let instances = [(0, 0), (1, 1), (2, 1), (3, 1), (4, 1), (5, 1), (0, 1)];
    let view = view(&[("A.erg", "A"), ("B.erg", "B")], &instances);
    frame(&view, &mut runtime, PLENTY);
    let report = frame(&view, &mut runtime, PLENTY);
    let per_turn = report.spent / 7;

    let mut events = EventQueue::new();
    events.push(khora_core::script::ScriptEvent::new(entity(0), "Hit"));
    let mut host = Host::new();
    let mut undelivered =
        run_behaviors(&view, &events, &mut runtime, &mut host, per_turn * 4).undelivered;
    for _ in 0..6 {
        let report = run_behaviors(&view, &undelivered, &mut runtime, &mut host, per_turn * 4);
        undelivered = report.undelivered;
    }
    for _ in 0..3 {
        let report = run_behaviors(&view, &undelivered, &mut runtime, &mut host, PLENTY);
        undelivered = report.undelivered;
    }
    assert_eq!(
        (hits(&runtime, 0, "A"), hits(&runtime, 0, "B")),
        (Some(1), Some(1)),
        "one `Hit`, heard once by each behavior of entity 0"
    );
}

/// **A behavior's cursor follows it when the view's program order changes.**
/// `First` on 0–9 and `Second` on 10–19, fuel for six turns: `First` runs
/// 0 1 2 and stops at 3. The next frame lists the programs the other way
/// round — a new behavior, a despawned first holder — and `First` still
/// carries on from 3, `Second` from 13.
#[test]
fn a_behavior_s_cursor_follows_it_when_the_program_order_changes() {
    let mut runtime = ScriptRuntime::new();
    runtime.add_program(
        "cursors.erg",
        build(
            "behavior First { int ticks = 0; void Update(float dt) { ticks += 1; } }
             behavior Second { int ticks = 0; void Update(float dt) { ticks += 1; } }",
        ),
    );
    let instances: Vec<(u32, u32)> = (0..10)
        .map(|i| (i, 0))
        .chain((10..20).map(|i| (i, 1)))
        .collect();
    let before = view(
        &[("cursors.erg", "First"), ("cursors.erg", "Second")],
        &instances,
    );
    frame(&before, &mut runtime, PLENTY);
    let per_turn = frame(&before, &mut runtime, PLENTY).spent / 20;
    let first = frame(&before, &mut runtime, per_turn * 6);
    let (mut f, mut s) = (ran(&first, "First"), ran(&first, "Second"));
    f.sort_unstable();
    s.sort_unstable();
    assert_eq!((f, s), (vec![0, 1, 2], vec![10, 11, 12]));

    let swapped: Vec<(u32, u32)> = (10..20)
        .map(|i| (i, 0))
        .chain((0..10).map(|i| (i, 1)))
        .collect();
    let after = view(
        &[("cursors.erg", "Second"), ("cursors.erg", "First")],
        &swapped,
    );
    let next = frame(&after, &mut runtime, per_turn * 6);
    let (mut f, mut s) = (ran(&next, "First"), ran(&next, "Second"));
    f.sort_unstable();
    s.sort_unstable();
    assert_eq!((f, s), (vec![3, 4, 5], vec![13, 14, 15]));
}

/// One call to the costly native a turn: every turn spends past any share
/// below 200, by about the same amount.
fn costly(name: &str) -> String {
    format!(
        "behavior {name} {{ int calls = 0;
             void Update(float dt) {{ float c = ShareCostly(); calls += 1; }} }}"
    )
}

/// **Two behaviors that overdraft alike run about as often.** `A` and `B`,
/// one instance each, every turn costing about 200 against a frame of 50:
/// each turn is a debt, and with equal shares each should run about half the
/// turns. A debt withholds only the behavior's own share, so `B`, after `A`,
/// runs on what `A` pays back of its debt, frame after frame, while its own
/// debt grows unpaid.
#[test]
fn two_behaviors_that_overdraft_alike_run_about_as_often() {
    let mut runtime = ScriptRuntime::new();
    runtime.add_program("A.erg", build(&costly("A")));
    runtime.add_program("B.erg", build(&costly("B")));
    let view = view(&[("A.erg", "A"), ("B.erg", "B")], &[(0, 0), (1, 1)]);
    frame(&view, &mut runtime, PLENTY);
    let before = (hits(&runtime, 0, "A"), hits(&runtime, 1, "B"));

    let mut spent = 0;
    for _ in 0..80 {
        spent += frame(&view, &mut runtime, 50).spent;
    }
    let a = hits(&runtime, 0, "A").unwrap_or(0) - before.0.unwrap_or(0);
    let b = hits(&runtime, 1, "B").unwrap_or(0) - before.1.unwrap_or(0);
    assert!(
        a.max(b) <= 2 * a.min(b) + 1,
        "`A` ran {a} turns and `B` {b} in 80 frames of 50 fuel ({spent} spent)"
    );
}

/// **A behavior that could not spend at the plan, brought back by an
/// arrival, spends within the frame's fuel.** `Flaky`'s only instance faults
/// and is disabled, so the frame hands it no share. Then its entity arrives
/// afresh — its component detached and attached again, which the flow hands
/// over as an arrival — with `armed` cleared: the instance is enabled again
/// within the frame and runs a loop. With 100 fuel the frame must stop near
/// 100, not at the 100 000 one behavior may spend at most.
#[test]
fn a_behavior_re_enabled_by_an_arrival_spends_within_the_frame_s_fuel() {
    let mut runtime = ScriptRuntime::new();
    runtime.add_program(
        "flaky.erg",
        build(
            "behavior Flaky { int armed = 1; int zero = 0; int n = 0;
                 void Update(float dt) {
                     if (armed == 1) { int x = 1 / zero; }
                     while (true) { n += 1; }
                 } }",
        ),
    );
    let mut view = view(&[("flaky.erg", "Flaky")], &[(0, 0)]);
    for _ in 0..3 {
        frame(&view, &mut runtime, PLENTY);
    }
    assert!(
        runtime
            .peek(entity(0), "Flaky")
            .is_some_and(|instance| instance.disabled),
        "`Flaky` faulted and is disabled"
    );

    view.instances[0].arrival = Some(khora_data::flow::ScriptArrival {
        fields: vec![("armed".to_owned(), khora_core::script::ScriptValue::Int(0))],
        observed: None,
    });
    let report = frame(&view, &mut runtime, 100);
    assert!(
        report.spent <= 1_000,
        "a frame of 100 fuel spent {} — {report:?}",
        report.spent
    );
}

/// **A rotation resumes after its cursor in entity order when the view does
/// not list its instances in that order.** One behavior attached by a
/// `Script` to some entities and by its component to others is listed one
/// kind after the other: here 5–9, then 0–4. Fuel for three turns runs 5 6 7,
/// then 8 9 0, stopping at 1. Entity 1 leaves; the next frame should start at
/// 2, the first after it — not at 5, the first listed with a greater index,
/// which leaves 2 3 4 out again.
#[test]
fn a_rotation_resumes_after_its_cursor_in_entity_order_when_the_view_is_not() {
    let mut runtime = ScriptRuntime::new();
    runtime.add_program("tick.erg", build(TICK));
    let all: Vec<(u32, u32)> = (5..10).chain(0..5).map(|i| (i, 0)).collect();
    let full = view(&[("tick.erg", "Tick")], &all);
    let per_turn = tick_turn(&full, &mut runtime, 10);

    let sorted = |report: &ScriptRunReport| {
        let mut ran = ran(report, "Tick");
        ran.sort_unstable();
        ran
    };
    assert_eq!(
        sorted(&frame(&full, &mut runtime, per_turn * 3)),
        vec![5, 6, 7]
    );
    assert_eq!(
        sorted(&frame(&full, &mut runtime, per_turn * 3)),
        vec![0, 8, 9]
    );

    let without_1: Vec<(u32, u32)> = all.iter().copied().filter(|&(i, _)| i != 1).collect();
    let next = view(&[("tick.erg", "Tick")], &without_1);
    assert_eq!(
        sorted(&frame(&next, &mut runtime, per_turn * 3)),
        vec![2, 3, 4],
        "the rotation carries on after the instance that left"
    );
}

/// **A rotation whose cursor's entity was recycled starts at the newcomer in
/// its slot.** Ten `Tick`s, fuel for three: the frame stops at 3. Entity 3 is
/// despawned and its index reused with a newer generation: the next frame
/// starts there — the first after the cursor — and runs it, 4 and 5.
#[test]
fn a_rotation_whose_cursor_was_recycled_starts_at_the_newer_generation() {
    let mut runtime = ScriptRuntime::new();
    runtime.add_program("tick.erg", build(TICK));
    let all: Vec<(u32, u32)> = (0..10).map(|i| (i, 0)).collect();
    let full = view(&[("tick.erg", "Tick")], &all);
    let per_turn = tick_turn(&full, &mut runtime, 10);
    frame(&full, &mut runtime, per_turn * 3);

    let mut recycled = full.clone();
    recycled.instances[3].entity.generation = 2;
    let report = frame(&recycled, &mut runtime, per_turn * 6);
    let mut ran: Vec<_> = report
        .state
        .iter()
        .filter(|update| update.behavior == "Tick")
        .map(|update| (update.entity.index, update.entity.generation))
        .collect();
    ran.sort_unstable();
    // The newcomer spends its first frame spawning; 4 5 6 7 8 run their turn.
    assert_eq!(ran.first(), Some(&(3, 2)), "{report:?}");
    assert!(ran.contains(&(4, 1)), "{report:?}");
    assert!(!ran.iter().any(|&(i, _)| i < 3), "{report:?}");
}

/// **All the fuel there is shares without overflow.** Three behaviors and
/// `u64::MAX` fuel: each runs every instance, nothing is deferred.
#[test]
fn the_largest_fuel_shares_without_overflow() {
    let mut runtime = ScriptRuntime::new();
    for name in ["A", "B", "C"] {
        runtime.add_program(
            format!("{name}.erg"),
            build(&format!(
                "behavior {name} {{ int ticks = 0; void Update(float dt) {{ ticks += 1; }} }}"
            )),
        );
    }
    let view = view(
        &[("A.erg", "A"), ("B.erg", "B"), ("C.erg", "C")],
        &[(0, 0), (1, 1), (2, 2), (3, 0)],
    );
    frame(&view, &mut runtime, u64::MAX);
    for _ in 0..3 {
        let report = frame(&view, &mut runtime, u64::MAX);
        assert_eq!((report.completed, report.deferred), (4, 0), "{report:?}");
    }
}
