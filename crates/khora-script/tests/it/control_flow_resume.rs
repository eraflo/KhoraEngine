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

//! A frame standing inside a `match` arm, an `if (var …)` branch or a
//! `while (var …)` body is found again after an edit elsewhere.
//!
//! Each is a named block like any other: a site inside it is named by the
//! statements around it, an arm by what it matches, so the edited code still
//! has the site, the frame is rebuilt there with the bound name carried as a
//! local, and the body finishes as the edited code would.

use std::collections::BTreeSet;

use khora_core::ecs::entity::EntityId;
use khora_core::script::{FrozenMachine, PendingBody};
use khora_script::arena::{Persisted, PersistentStore};
use khora_script::bytecode::init_name;
use khora_script::vm::{resume, ResumeTier, SiteKind, Suspension};
use khora_script::{check, compile, lex, parse, Function, Host, Machine, Program, Run, Value};

const SUBJECT: EntityId = EntityId {
    index: 2,
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

/// `Guard`, with the same fields in every version, and `members`.
fn guard(members: &str) -> Program {
    build(&format!(
        "behavior Guard {{
             int starts = 0;
             int landed = 0;
             int extra = 0;
             int left = 2;
             int? Next() {{
                 if (left == 0) {{ return null; }}
                 left -= 1;
                 return left;
             }}
             {members}
         }}"
    ))
}

/// Runs `machine` to the end, every `await` treated as elapsed.
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

/// `Guard.Attack(args)` of `original` stopped at its first `await` and
/// frozen, resumed into `edited` on the same instance and run to its end:
/// the frozen machine, the tier, and `(starts, landed, extra)` afterwards.
fn attack_resumed(
    original: &Program,
    edited: &Program,
    args: &[Value],
) -> (FrozenMachine, ResumeTier, [Option<i64>; 3]) {
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
    (
        frozen,
        tier,
        [field("starts"), field("landed"), field("extra")],
    )
}

/// The site the innermost frame of `frozen` stands at, in `program`.
fn innermost_site<'a>(program: &'a Program, frozen: &FrozenMachine) -> &'a khora_script::vm::Site {
    let frame = frozen.frames.last().expect("a frame");
    program
        .function(&frame.function)
        .and_then(|function| function.sites.iter().find(|site| site.name == frame.site))
        .unwrap_or_else(|| panic!("`{}` is a site of its function", frame.site))
}

fn local_names(site: &khora_script::vm::Site) -> BTreeSet<String> {
    site.locals.iter().map(|local| local.name.clone()).collect()
}

// ─── Rebuilt inside the new blocks ──────────────────────────────────────────

/// **Inside a `match` arm.** The body awaits in the `int t` arm; the edit
/// changes the `null` arm only. The frame is rebuilt at its site in the arm —
/// `t` carried as a local of type `int` — and the arm finishes with the 5 it
/// matched, the statement before the `match` counted once.
#[test]
fn a_frame_inside_a_match_arm_rebuilds_after_an_edit_elsewhere() {
    let attack = |null_arm: &str| {
        guard(&format!(
            "async void Attack(int? target) {{
                 starts += 1;
                 match (target) {{
                     int t => {{
                         await 1.0s;
                         landed = t;
                     }}
                     null => {{ {null_arm} }}
                 }}
             }}"
        ))
    };
    let original = attack("landed = -1;");
    let edited = attack("landed = -2; extra += 1;");

    let (frozen, tier, fields) = attack_resumed(&original, &edited, &[Value::Int(5)]);
    let site = innermost_site(&original, &frozen);

    assert!(
        site.name.contains("/arm"),
        "the await stands in a named arm: `{}`",
        site.name
    );
    assert!(
        site.locals
            .iter()
            .any(|local| local.name == "t" && local.ty == "int"),
        "the bound name is a local of the arm: {:?}",
        site.locals
    );
    assert_eq!(
        (tier, fields),
        (ResumeTier::Rebuilt, [Some(1), Some(5), Some(0)]),
        "(tier, [starts, landed, extra])"
    );
}

/// An edit later in the same arm is run, with the bound value carried.
#[test]
fn a_frame_inside_a_match_arm_runs_the_edited_rest_of_its_arm() {
    let attack = |after: &str| {
        guard(&format!(
            "async void Attack(int? target) {{
                 starts += 1;
                 match (target) {{
                     int t => {{
                         await 1.0s;
                         landed = t{after};
                     }}
                     _ => {{ landed = -1; }}
                 }}
             }}"
        ))
    };
    let (_, tier, fields) = attack_resumed(&attack(""), &attack(" + 100"), &[Value::Int(5)]);

    assert_eq!(
        (tier, fields),
        (ResumeTier::Rebuilt, [Some(1), Some(105), Some(0)]),
        "(tier, [starts, landed, extra])"
    );
}

/// **Inside an `if (var …)` then-branch.** The edit is in the `else`; the
/// frame is rebuilt in the then-branch with the narrowed `t`.
#[test]
fn a_frame_inside_an_if_var_branch_rebuilds_after_an_edit_elsewhere() {
    let attack = |otherwise: &str| {
        guard(&format!(
            "async void Attack(int? target) {{
                 starts += 1;
                 if (var t = target) {{
                     await 1.0s;
                     landed = t;
                 }} else {{
                     {otherwise}
                 }}
             }}"
        ))
    };
    let original = attack("landed = -1;");
    let edited = attack("landed = -2; extra += 1;");

    let (frozen, tier, fields) = attack_resumed(&original, &edited, &[Value::Int(5)]);

    assert!(
        local_names(innermost_site(&original, &frozen)).contains("t"),
        "the narrowed name is a local of the branch"
    );
    assert_eq!(
        (tier, fields),
        (ResumeTier::Rebuilt, [Some(1), Some(5), Some(0)]),
        "(tier, [starts, landed, extra])"
    );
}

/// **Inside a `while (var …)` body.** Frozen on the first turn (`t` is 1);
/// the edit is after the loop. Rebuilt, the loop goes on to take the next
/// value from `Next`, and the edited statement after it runs.
#[test]
fn a_frame_inside_a_while_var_body_rebuilds_after_an_edit_elsewhere() {
    let attack = |after: &str| {
        guard(&format!(
            "async void Attack() {{
                 starts += 1;
                 while (var t = Next()) {{
                     await 1.0s;
                     landed = landed + t + 1;
                 }}
                 {after}
             }}"
        ))
    };
    let original = attack("");
    let edited = attack("extra += 1;");

    let (frozen, tier, fields) = attack_resumed(&original, &edited, &[]);

    assert!(
        local_names(innermost_site(&original, &frozen)).contains("t"),
        "the bound name is a local of the body"
    );
    // `Next` yields 1, then 0: landed = (1 + 1) + (0 + 1).
    assert_eq!(
        (tier, fields),
        (ResumeTier::Rebuilt, [Some(1), Some(3), Some(1)]),
        "(tier, [starts, landed, extra])"
    );
}

// ─── Names ──────────────────────────────────────────────────────────────────

fn awaits(function: &Function) -> Vec<String> {
    function
        .sites
        .iter()
        .filter(|site| site.kind == SiteKind::Await)
        .map(|site| site.name.clone())
        .collect()
}

/// **Two arms saying the same thing are two sites**, each named in its arm —
/// and an edit inside one arm renames nothing in the other.
#[test]
fn match_arms_saying_the_same_thing_have_distinct_sites() {
    let attack = |null_tail: &str| {
        guard(&format!(
            "async void Attack(int? target) {{
                 match (target) {{
                     int t => {{ await 1.0s; landed += 1; }}
                     null => {{ await 1.0s; landed += 1; {null_tail} }}
                 }}
             }}"
        ))
    };
    let before = attack("");
    let after = attack("extra += 1;");
    let function = before.function("Guard.Attack").expect("Attack");
    let names = awaits(function);

    assert_eq!(names.len(), 2, "the premise: two awaits");
    assert_ne!(names[0], names[1], "one name per arm: {names:?}");
    assert!(
        names.iter().all(|name| name.contains("/arm")),
        "each await is named in its arm: {names:?}"
    );
    let all: BTreeSet<&String> = function.sites.iter().map(|site| &site.name).collect();
    assert_eq!(
        all.len(),
        function.sites.len(),
        "no two sites share a name: {:?}",
        function.sites
    );
    assert_eq!(
        names,
        awaits(after.function("Guard.Attack").expect("Attack")),
        "an edit in the `null` arm, after its await, renames neither"
    );
}

// ─── Any cut, rebuilt ───────────────────────────────────────────────────────

/// Runs `machine` to its end in one go.
fn result_of(mut machine: Machine, program: &Program) -> Result<Value, String> {
    let mut host = Host::new();
    for _ in 0..1000 {
        match machine.run(program, &mut host, u64::MAX) {
            Run::Completed => return Ok(machine.result()),
            Run::Suspended(_) => host.awaiting = None,
            Run::Faulted(fault) => return Err(format!("{fault:?}")),
        }
    }
    Err("still running after 1000 resumes".to_owned())
}

/// **Any cut, rebuilt, finishes as the edited code would.** For every fuel
/// slice, every cut of a body with a `match` in a loop, an `if (var …)` with a
/// `continue`, a `while (var …)` and a `break` is resumed into an edit of its
/// last statement. Pure code: whatever tier it lands on, the result is the
/// edited program's — and some cut inside an arm is rebuilt.
#[test]
fn every_cut_through_control_flow_resumed_into_edited_code_finishes_with_the_edited_result() {
    let source = |tail: &str| {
        format!(
            "fn int? Odd(int i) {{
                 if (i % 2 == 0) {{ return null; }}
                 return i;
             }}
             fn int F() {{
                 int total = 0;
                 for (int i = 0; i < 6; i = i + 1) {{
                     match (Odd(i)) {{
                         int o => {{ total = total + o * 2; }}
                         null => {{ total = total + 1; }}
                     }}
                     if (var again = Odd(i + 1)) {{
                         total = total + again;
                     }} else {{
                         continue;
                     }}
                     if (total > 40) {{ break; }}
                 }}
                 int k = 1;
                 while (var z = Odd(k)) {{
                     total = total + z;
                     k = k + 2;
                     if (k > 5) {{ break; }}
                 }}
                 {tail}
             }}"
        )
    };
    let original = build(&source("return total;"));
    let edited = build(&source("int bonus = 1000;\n return total + bonus;"));
    let expected = result_of(Machine::new(&edited, "F", &[]).expect("F"), &edited);
    assert!(expected.is_ok(), "{expected:?}");

    let mut whole = Machine::new(&original, "F", &[]).expect("F");
    let (_, cost) = whole.run_counting(&original, &mut Host::new(), u64::MAX);
    let mut rebuilt = 0;
    let mut rebuilt_in_an_arm = 0;
    for slice in 1..=cost {
        let mut machine = Machine::new(&original, "F", &[]).expect("F");
        let mut host = Host::new();
        loop {
            match machine.run(&original, &mut host, slice) {
                Run::Completed => break,
                Run::Suspended(Suspension::OutOfFuel) => {
                    let frozen = machine
                        .freeze(&original, PendingBody::Update)
                        .unwrap_or_else(|| panic!("slice {slice}: a cut freezes"));
                    let sites: Vec<&String> = frozen.frames.iter().map(|f| &f.site).collect();
                    let (resumed, tier) = resume(&frozen, original.fingerprint(), &edited)
                        .unwrap_or_else(|why| panic!("slice {slice}: abandoned {why:?}"));
                    if tier == ResumeTier::Rebuilt {
                        rebuilt += 1;
                        if sites.iter().any(|site| site.contains("/arm")) {
                            rebuilt_in_an_arm += 1;
                        }
                    }
                    assert_eq!(
                        result_of(resumed, &edited),
                        expected,
                        "slice {slice}, frozen at {sites:?}, resumed {tier:?}"
                    );
                }
                other => panic!("slice {slice}: {other:?}"),
            }
        }
    }
    assert!(rebuilt > 0, "some cut was rebuilt");
    assert!(rebuilt_in_an_arm > 0, "some cut inside an arm was rebuilt");
}
