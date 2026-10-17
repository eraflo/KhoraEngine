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

//! Text built by `$"…"` held while a body is suspended, and a body suspended
//! inside a hole.
//!
//! The frame arena is emptied between frames. A string a suspended body still
//! holds must read back as itself after an `await`, a fuel cut or a save —
//! including a hole already evaluated while a later hole waits.

use khora_core::script::{FrozenMachine, FrozenValue, PendingBody};
use khora_script::arena::Object;
use khora_script::vm::{resume, Suspension};
use khora_script::{Host, Machine, Program, Run};

use super::build;

/// Puts other text at the first indices of the arena.
fn occupy(host: &mut Host) {
    for text in ["other", "frame's", "x9", "hp 9", "not", "yours"] {
        host.arena
            .alloc(Object::Str(text.to_owned()))
            .expect("the arena has room");
    }
}

/// Ends the frame as the lane does and fills the next one with other text.
fn next_frame(host: &mut Host) {
    host.awaiting = None;
    host.end_frame();
    occupy(host);
}

fn result_text(machine: &Machine, program: &Program, host: &Host) -> Result<String, String> {
    machine
        .resolve_str(machine.result(), program, host)
        .map(str::to_owned)
        .map_err(|fault| format!("{fault:?}"))
}

/// Runs to the end, a frame per suspension; the text it returned.
fn finish(
    machine: &mut Machine,
    program: &Program,
    host: &mut Host,
    fuel: u64,
) -> Result<String, String> {
    for _ in 0..100_000 {
        match machine.run(program, host, fuel) {
            Run::Suspended(_) => next_frame(host),
            Run::Completed => return result_text(machine, program, host),
            Run::Faulted(fault) => return Err(format!("{fault:?}")),
        }
    }
    Err("never finished".to_owned())
}

// ─── Text held across a suspension ──────────────────────────────────────────

/// Text built by `$"…"`, held across an `await`, then returned — with other
/// text built in the frame that resumes it.
const GREET: &str = r#"behavior Guard {
                           async string Greet() {
                               int n = 3;
                               string s = $"hp {n}";
                               await 1.0s;
                               string noise = $"hp {n + 6}";
                               return s;
                           }
                       }"#;

fn greet_stopped(program: &Program, host: &mut Host) -> Machine {
    let mut machine = Machine::new(program, "Guard.Greet", &[]).expect("the member exists");
    assert_eq!(
        machine.run(program, host, u64::MAX),
        Run::Suspended(Suspension::Awaiting),
        "the body stopped at its await"
    );
    machine
}

#[test]
fn interpolated_text_survives_an_await() {
    let program = build(GREET);
    let mut host = Host::new();
    let mut machine = greet_stopped(&program, &mut host);
    next_frame(&mut host);
    assert_eq!(
        finish(&mut machine, &program, &mut host, u64::MAX),
        Ok("hp 3".to_owned())
    );
}

/// A save mid-`await` writes the text itself, and the machine thawed from it,
/// in an arena holding other text, returns it.
#[test]
fn interpolated_text_survives_a_save() {
    let program = build(GREET);
    let mut host = Host::new();
    let machine = greet_stopped(&program, &mut host);
    let frozen = machine
        .freeze(&program, PendingBody::Sequence)
        .expect("a machine of this program freezes");
    assert!(
        frozen
            .registers
            .contains(&FrozenValue::Text("hp 3".to_owned())),
        "the built text is written as its text: {:?}",
        frozen.registers
    );

    let json = serde_json::to_string(&frozen).expect("serialises");
    let loaded: FrozenMachine = serde_json::from_str(&json).expect("parses");
    let mut thawed = Machine::thaw(&loaded, &program).expect("thaws into its own program");
    let mut later = Host::new();
    occupy(&mut later);
    assert_eq!(
        finish(&mut thawed, &program, &mut later, u64::MAX),
        Ok("hp 3".to_owned())
    );
}

// ─── A body stopped in a hole ───────────────────────────────────────────────

/// `Greet` waits inside its second hole, the first already evaluated into
/// text built at run time; `tail` is appended to what it returns.
fn waiting_in_a_hole(tail: &str) -> Program {
    build(&format!(
        r#"behavior Guard {{
               async int Wait() {{ await 1.0s; return 7; }}
               async string Greet() {{
                   int n = 3;
                   string s = $"{{$"x{{n}}"}}-{{Wait()}}-{{n + 1}}";
                   return s{tail};
               }}
           }}"#
    ))
}

/// **Waiting in a hole.** Stopped at the `await` its second hole reaches, the
/// body resumes in a later frame — and from a save — with the first hole's
/// text intact and the holes after it still to come.
#[test]
fn a_body_waiting_in_a_hole_resumes_with_the_holes_before_it() {
    let program = waiting_in_a_hole("");
    let mut host = Host::new();
    let mut machine = Machine::new(&program, "Guard.Greet", &[]).expect("Greet");
    assert_eq!(
        machine.run(&program, &mut host, u64::MAX),
        Run::Suspended(Suspension::Awaiting)
    );
    assert_eq!(
        machine.depth(),
        2,
        "the premise: stopped inside the hole's call"
    );

    let frozen = machine
        .freeze(&program, PendingBody::Sequence)
        .expect("freezes");
    let mut thawed = Machine::thaw(&frozen, &program).expect("thaws");
    let mut later = Host::new();
    occupy(&mut later);
    assert_eq!(
        finish(&mut thawed, &program, &mut later, u64::MAX),
        Ok("x3-7-4".to_owned()),
        "from a save"
    );

    next_frame(&mut host);
    assert_eq!(
        finish(&mut machine, &program, &mut host, u64::MAX),
        Ok("x3-7-4".to_owned()),
        "in a later frame"
    );
}

/// **Every fuel cut.** At every slice, the arena emptied between runs, the
/// body returns what it returns run whole.
#[test]
fn a_body_cut_by_fuel_anywhere_in_its_holes_returns_the_same_text() {
    let program = waiting_in_a_hole("");
    let mut wrong = Vec::new();
    for slice in 1..=60u64 {
        let mut machine = Machine::new(&program, "Guard.Greet", &[]).expect("Greet");
        let mut host = Host::new();
        let got = finish(&mut machine, &program, &mut host, slice);
        if got.as_deref() != Ok("x3-7-4") {
            wrong.push(format!("slice {slice}: {got:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// **Every fuel cut, then an edit.** Frozen at each cut and resumed into a
/// program whose code after the interpolation changed, every run returns the
/// edited text — the holes already evaluated carried, not lost or misread.
#[test]
fn a_body_frozen_in_its_holes_resumes_into_an_edited_program() {
    let original = waiting_in_a_hole("");
    let edited = waiting_in_a_hole(r#" + "!""#);

    // What the whole body costs, its `await` included.
    let mut whole = Machine::new(&original, "Guard.Greet", &[]).expect("Greet");
    let mut host = Host::new();
    let mut cost = 0;
    for _ in 0..100 {
        let (run, spent) = whole.run_counting(&original, &mut host, u64::MAX);
        cost += spent;
        match run {
            Run::Completed => break,
            Run::Suspended(_) => host.awaiting = None,
            Run::Faulted(fault) => panic!("run whole, it faulted: {fault:?}"),
        }
    }

    let mut wrong = Vec::new();
    let mut cuts = 0;
    for slice in 1..=cost.max(1) {
        let mut machine = Machine::new(&original, "Guard.Greet", &[]).expect("Greet");
        let mut host = Host::new();
        for _ in 0..10_000 {
            match machine.run(&original, &mut host, slice) {
                Run::Completed => break,
                Run::Suspended(Suspension::Awaiting) => host.awaiting = None,
                Run::Suspended(Suspension::OutOfFuel) => {
                    cuts += 1;
                    let frozen = machine
                        .freeze(&original, PendingBody::Sequence)
                        .expect("a machine of its program freezes");
                    let sites: Vec<String> = frozen
                        .frames
                        .iter()
                        .map(|frame| frame.site.clone())
                        .collect();
                    let got = match resume(&frozen, original.fingerprint(), &edited) {
                        Ok((mut resumed, tier)) => {
                            let mut later = Host::new();
                            occupy(&mut later);
                            finish(&mut resumed, &edited, &mut later, u64::MAX)
                                .map_err(|why| format!("{why} ({tier:?})"))
                        }
                        Err(abandoned) => Err(format!("abandoned {abandoned:?}")),
                    };
                    if got.as_deref() != Ok("x3-7-4!") {
                        wrong.push(format!("slice {slice} at {sites:?}: {got:?}"));
                    }
                }
                Run::Faulted(fault) => {
                    wrong.push(format!("slice {slice}: faulted {fault:?}"));
                    break;
                }
            }
        }
    }
    assert!(cuts > 0, "the premise: some slice cuts the body");
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
