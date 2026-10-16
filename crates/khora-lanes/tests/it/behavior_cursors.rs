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

//! **Behaviors keep a budget cursor per behavior** — a program, `(module,
//! behavior)`. The instances of one behavior share its cursor, whichever
//! entities hold them; two behaviors never share one, even when they live in
//! one module or carry one name in two. Each behavior is handed its fair share
//! of the frame's fuel, in the view's program order.

use khora_core::ecs::entity::EntityId;
use khora_core::script::EventQueue;
use khora_data::flow::{ScriptInstance, ScriptProgram, ScriptView};
use khora_lanes::script_lane::{run_behaviors, ScriptRunReport, ScriptRuntime};
use khora_script::vm::Program;
use khora_script::{check, compile, lex, parse, Host};

/// Far more fuel than any frame below spends.
const PLENTY: u64 = 10_000_000;

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

/// A hand-built view: `programs` in their order, then each instance as
/// `(entity index, program index)`, in the order given — entity order, as the
/// flow lists each behavior's instances.
fn view_of(programs: &[(&str, &str)], instances: &[(u32, u32)]) -> ScriptView {
    ScriptView {
        fixed_steps: 0,
        fixed_delta_seconds: 0.0,
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
                row: Vec::new(),
            })
            .collect(),
    }
}

/// One frame of the lane over `view` with `fuel`.
fn frame(view: &ScriptView, runtime: &mut ScriptRuntime, fuel: u64) -> ScriptRunReport {
    let mut host = Host::new();
    run_behaviors(view, &EventQueue::new(), runtime, &mut host, fuel)
}

/// The entities, by index, whose `behavior` did work this frame, in the order
/// the frame ran them.
fn ran(report: &ScriptRunReport, behavior: &str) -> Vec<u32> {
    report
        .state
        .iter()
        .filter(|update| update.behavior == behavior)
        .map(|update| update.entity.index)
        .collect()
}

/// Two frames with plenty, so every instance has spawned and the turns that
/// follow all cost the same; returns what one turn costs, checked to be exact.
fn settle(view: &ScriptView, runtime: &mut ScriptRuntime) -> u64 {
    frame(view, runtime, PLENTY);
    let report = frame(view, runtime, PLENTY);
    let turns = view.instances.len() as u64;
    assert_eq!(report.completed as u64, turns, "{report:?}");
    assert_eq!(
        report.spent % turns,
        0,
        "every turn costs the same: {} over {turns}",
        report.spent
    );
    report.spent / turns
}

const TICK: &str = "void Update(float dt) { ticks += 1; }";

fn two_behaviors_one_module() -> ScriptRuntime {
    let mut runtime = ScriptRuntime::new();
    runtime.add_program(
        "cursors.erg",
        build(&format!(
            "behavior First {{ int ticks = 0; {TICK} }}
             behavior Second {{ int ticks = 0; {TICK} }}"
        )),
    );
    runtime
}

/// **Two behaviors of one module resume from separate cursors.**
/// `First` runs on entities 0–9 and `Second` on 10–19, so a single cursor
/// over the view would spend a frame's fuel on one block and leave the other
/// out. With fuel for six turns each behavior takes three, every frame, from
/// where it stopped — so all twenty run within four frames.
#[test]
fn two_behaviors_of_one_module_resume_from_separate_cursors() {
    let instances: Vec<(u32, u32)> = (0..10)
        .map(|i| (i, 0))
        .chain((10..20).map(|i| (i, 1)))
        .collect();
    let view = view_of(
        &[("cursors.erg", "First"), ("cursors.erg", "Second")],
        &instances,
    );
    let mut runtime = two_behaviors_one_module();
    let per_turn = settle(&view, &mut runtime);

    let mut first: Vec<u32> = Vec::new();
    let mut second: Vec<u32> = Vec::new();
    for at in 0..4 {
        let report = frame(&view, &mut runtime, per_turn * 6);
        let (f, s) = (ran(&report, "First"), ran(&report, "Second"));
        assert_eq!(
            (f.len(), s.len()),
            (3, 3),
            "frame {at}: each behavior takes half the fuel — First ran {f:?}, Second ran {s:?}"
        );
        first.extend(f);
        second.extend(s);
    }
    for index in 0..10 {
        assert!(
            first.contains(&index),
            "First on {index} never ran: {first:?}"
        );
    }
    for index in 10..20 {
        assert!(
            second.contains(&index),
            "Second on {index} never ran: {second:?}"
        );
    }
}

/// **Every instance of one behavior shares its cursor, whichever
/// entities hold it.** `First` holds entities 0–4 and 10–14, `Second` 5–9 and
/// 15–19. With fuel for six turns, `First` sweeps its own ten in entity order,
/// three a frame, picking up at the first one the last frame had no fuel for:
/// 0 1 2, then 3 4 10, then 11 12 13, then 14 0 1 — and `Second` likewise
/// through its own.
#[test]
fn the_instances_of_one_behavior_share_its_cursor() {
    let first_holds: Vec<u32> = (0..5).chain(10..15).collect();
    let instances: Vec<(u32, u32)> = (0..20)
        .map(|i| (i, if first_holds.contains(&i) { 0 } else { 1 }))
        .collect();
    let view = view_of(
        &[("cursors.erg", "First"), ("cursors.erg", "Second")],
        &instances,
    );
    let mut runtime = two_behaviors_one_module();
    let per_turn = settle(&view, &mut runtime);

    let expected_first = [
        vec![0, 1, 2],
        vec![3, 4, 10],
        vec![11, 12, 13],
        vec![14, 0, 1],
    ];
    let expected_second = [
        vec![5, 6, 7],
        vec![8, 9, 15],
        vec![16, 17, 18],
        vec![19, 5, 6],
    ];
    for (at, (want_first, want_second)) in expected_first.iter().zip(&expected_second).enumerate() {
        let report = frame(&view, &mut runtime, per_turn * 6);
        let mut first = ran(&report, "First");
        let mut second = ran(&report, "Second");
        // The set a frame ran; the wrap-around is checked by membership.
        first.sort_unstable();
        second.sort_unstable();
        let mut want_first = want_first.clone();
        let mut want_second = want_second.clone();
        want_first.sort_unstable();
        want_second.sort_unstable();
        assert_eq!(first, want_first, "frame {at}: First");
        assert_eq!(second, want_second, "frame {at}: Second");
    }
}

/// **Two behaviors with one name, in two modules, are two behaviors.**
/// A program is `(module, behavior)`: `First` of `a.erg` on entities 0–9 and
/// `First` of `b.erg` on 10–19 each keep their own cursor and their own share,
/// so with fuel for six turns each runs three a frame.
#[test]
fn same_named_behaviors_of_two_modules_get_separate_cursors() {
    let mut runtime = ScriptRuntime::new();
    let source = format!("behavior First {{ int ticks = 0; {TICK} }}");
    runtime.add_program("a.erg", build(&source));
    runtime.add_program("b.erg", build(&source));
    let instances: Vec<(u32, u32)> = (0..10)
        .map(|i| (i, 0))
        .chain((10..20).map(|i| (i, 1)))
        .collect();
    let view = view_of(&[("a.erg", "First"), ("b.erg", "First")], &instances);
    let per_turn = settle(&view, &mut runtime);

    let mut seen: Vec<u32> = Vec::new();
    for at in 0..4 {
        let report = frame(&view, &mut runtime, per_turn * 6);
        let ran = ran(&report, "First");
        let of_a = ran.iter().filter(|&&i| i < 10).count();
        let of_b = ran.iter().filter(|&&i| i >= 10).count();
        assert_eq!(
            (of_a, of_b),
            (3, 3),
            "frame {at}: a.erg's First and b.erg's First take half each — ran {ran:?}"
        );
        seen.extend(ran);
    }
    for index in 0..20 {
        assert!(seen.contains(&index), "entity {index} never ran: {seen:?}");
    }
}

/// `Worker` on entity 0 is `heavy`: its `Update` loops far longer than a
/// frame's fuel. The others are one increment.
const WORKERS: &str = "behavior Worker {
    int heavy = 0;
    int turns = 0;
    void Update(float dt) {
        turns += 1;
        if (heavy == 1) { int n = 0; while (n < 20000) { n += 1; } }
    }
}";

/// **One instance's long body does not keep its siblings out.** Three
/// `Worker`s; entity 0's `Update` needs dozens of frames of fuel, entities 1
/// and 2 a few instructions. With fuel for many light turns a frame, the cut
/// body carries on when the behavior's rotation comes back to it — and in the
/// meantime the light instances keep taking their turns, rather than waiting
/// for the heavy one to finish.
#[test]
fn a_body_cut_by_fuel_does_not_keep_its_siblings_out_until_it_finishes() {
    use super::scheduled_scene::{frame as run, int_of, runtime_of, view};
    use khora_core::script::ScriptValue;

    let module = "workers.erg";
    let mut scene = view(module, &[("Worker", &[0, 1, 2])], 1.0 / 64.0);
    scene.instances[0].row = vec![("heavy".to_owned(), ScriptValue::Int(1))];
    let mut runtime = runtime_of(module, WORKERS);

    let fuel = 2_000;
    for _ in 0..10 {
        run(&scene, &mut runtime, fuel);
    }
    assert_eq!(
        int_of(&runtime, module, 0, "Worker", "heavy"),
        1,
        "set-up: entity 0 is the heavy one"
    );
    assert_eq!(
        int_of(&runtime, module, 0, "Worker", "turns"),
        1,
        "set-up: the heavy body has not finished in ten frames"
    );
    for index in [1, 2] {
        let turns = int_of(&runtime, module, index, "Worker", "turns");
        assert!(
            turns >= 3,
            "entity {index}: a light instance took {turns} turns in ten frames while its sibling's long body ran"
        );
    }
}
