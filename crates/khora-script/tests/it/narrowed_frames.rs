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

//! Frames standing where a `match` or an `if (var …)` chain keeps values
//! nobody named: nested matches cut at every slice, a `null` arm whose
//! subject temporary is still live at its await, and the second branch of an
//! `else if (var …)` chain, each rebuilt or resumed with the right values.

use khora_core::ecs::entity::EntityId;
use khora_core::script::PendingBody;
use khora_script::arena::{Persisted, PersistentStore};
use khora_script::bytecode::init_name;
use khora_script::vm::{resume, ResumeTier, Suspension};
use khora_script::{check, compile, lex, parse, Host, Machine, Program, Run, Value};

const SUBJECT: EntityId = EntityId {
    index: 5,
    generation: 1,
};

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

fn run(program: &Program, entry: &str, args: &[Value]) -> Value {
    let mut machine = Machine::new(program, entry, args).expect("the entry exists");
    match machine.run(program, &mut Host::new(), 1_000_000) {
        Run::Completed => machine.result(),
        other => panic!("`{entry}` did not finish: {other:?}"),
    }
}

/// Every slice of `entry(args)`, each cut frozen and resumed exactly: the
/// result is the uninterrupted one.
fn survives_every_cut(program: &Program, entry: &str, args: &[Value], expected: &Value) {
    let mut whole = Machine::new(program, entry, args).expect("the entry exists");
    let (_, cost) = whole.run_counting(program, &mut Host::new(), u64::MAX);
    for slice in 1..=cost.max(60) {
        let mut host = Host::new();
        let mut machine = Machine::new(program, entry, args).expect("the entry exists");
        loop {
            match machine.run(program, &mut host, slice) {
                Run::Completed => break,
                Run::Suspended(Suspension::Awaiting) => host.awaiting = None,
                Run::Suspended(Suspension::OutOfFuel) => {
                    let frozen = machine
                        .freeze(program, PendingBody::Update)
                        .unwrap_or_else(|| panic!("slice {slice}: a cut freezes"));
                    let (resumed, tier) = resume(&frozen, program.fingerprint(), program)
                        .unwrap_or_else(|why| panic!("slice {slice}: abandoned {why:?}"));
                    assert_eq!(tier, ResumeTier::Exact, "slice {slice}");
                    machine = resumed;
                }
                Run::Faulted(fault) => panic!("slice {slice}: faulted {fault:?}"),
            }
        }
        assert_eq!(&machine.result(), expected, "slice {slice}");
    }
}

/// **Nested matches share registers safely.** Each arm declares locals over
/// the registers its subject was tested in, and holds an inner `match`; all
/// four cases give their own result, through every cut.
#[test]
fn nested_matches_take_each_case_through_every_cut() {
    let program = build(
        "fn int F(int? a, int? b) {
             int r = 0;
             match (a) {
                 int x => {
                     int keep = x + 1;
                     match (b) {
                         int y => { int z = y * 2; r = keep * 100 + z; }
                         null => { r = keep; }
                     }
                 }
                 null => {
                     int w = 7;
                     match (b) {
                         int y => { r = y + w; }
                         _ => { r = -1 - w; }
                     }
                 }
             }
             return r;
         }",
    );
    let cases = [
        (Value::Int(3), Value::Int(5), 410),
        (Value::Int(3), Value::Null, 4),
        (Value::Null, Value::Int(5), 12),
        (Value::Null, Value::Null, -8),
    ];
    for (a, b, expected) in cases {
        let args = [a, b];
        assert_eq!(run(&program, "F", &args), Value::Int(expected), "{args:?}");
        survives_every_cut(&program, "F", &args, &Value::Int(expected));
    }
}

// ─── Rebuilt after an edit elsewhere ────────────────────────────────────────

fn guard(members: &str) -> Program {
    build(&format!(
        "behavior Guard {{
             int starts = 0;
             int landed = 0;
             int left = 1;
             int? Next() {{
                 if (left == 0) {{ return null; }}
                 left -= 1;
                 return 40 + left;
             }}
             {members}
         }}"
    ))
}

fn finish(machine: &mut Machine, program: &Program, host: &mut Host) {
    for _ in 0..1000 {
        match machine.run(program, host, u64::MAX) {
            Run::Completed => return,
            Run::Suspended(_) => host.awaiting = None,
            Run::Faulted(fault) => panic!("faulted: {fault:?}"),
        }
    }
    panic!("still running after 1000 resumes");
}

/// `Guard.Attack(args)` of `original` stopped at its await, resumed into
/// `edited`: the tier and `(starts, landed)` once it finished.
fn attack_resumed(
    original: &Program,
    edited: &Program,
    args: &[Value],
) -> (ResumeTier, [Option<i64>; 2]) {
    let layout = original.layout("Guard").expect("a layout");
    let mut host = Host::new()
        .with_fields(PersistentStore::with_slots(layout.slot_count()))
        .for_entity(SUBJECT);
    let mut init = Machine::new(original, &init_name("Guard"), &[]).expect("defaults");
    finish(&mut init, original, &mut host);
    let mut machine = Machine::new(original, "Guard.Attack", args).expect("Attack");
    assert_eq!(
        machine.run(original, &mut host, u64::MAX),
        Run::Suspended(Suspension::Awaiting),
        "the body stopped at its await"
    );
    host.awaiting = None;
    let frozen = machine
        .freeze(original, PendingBody::Sequence)
        .expect("a machine of its program freezes");
    let (mut machine, tier) = resume(&frozen, original.fingerprint(), edited)
        .unwrap_or_else(|abandoned| panic!("abandoned: {abandoned:?}"));
    let mut host = Host::new()
        .with_fields(host.fields.clone())
        .for_entity(SUBJECT);
    finish(&mut machine, edited, &mut host);
    let layout = edited.layout("Guard").expect("a layout");
    let field = |name: &str| match host.fields.get(layout.slot_of(name)?) {
        Some(Persisted::Scalar(Value::Int(value))) => Some(*value),
        _ => None,
    };
    (tier, [field("starts"), field("landed")])
}

/// **A `null` arm of a called subject.** The subject's temporary is still
/// live at the arm's await; the edit is in the other arm. Rebuilt, the arm
/// finishes and the statements before the `match` are not run again.
#[test]
fn a_frame_in_a_null_arm_of_a_called_subject_rebuilds() {
    let attack = |other: &str| {
        guard(&format!(
            "async void Attack() {{
                 starts += 1;
                 left = 0;
                 match (Next()) {{
                     int t => {{ {other} }}
                     null => {{ await 1.0s; landed = 77; }}
                 }}
             }}"
        ))
    };
    assert_eq!(
        attack_resumed(&attack("landed = 1;"), &attack("landed = 2;"), &[]),
        (ResumeTier::Rebuilt, [Some(1), Some(77)]),
        "(tier, [starts, landed])"
    );
}

/// **The second link of an `else if (var …)` chain.** Each binding's present
/// type is recorded for its own local; the edit is in the first branch. The
/// frame is rebuilt in the second, `y` carried with the value it bound.
#[test]
fn a_frame_in_an_else_if_var_branch_rebuilds() {
    let attack = |first: &str| {
        guard(&format!(
            "async void Attack(int? a, int? b) {{
                 starts += 1;
                 if (var x = a) {{
                     {first}
                 }} else if (var y = b) {{
                     await 1.0s;
                     landed = y;
                 }}
             }}"
        ))
    };
    assert_eq!(
        attack_resumed(
            &attack("landed = x;"),
            &attack("landed = x + 1;"),
            &[Value::Null, Value::Int(6)]
        ),
        (ResumeTier::Rebuilt, [Some(1), Some(6)]),
        "(tier, [starts, landed])"
    );
}
