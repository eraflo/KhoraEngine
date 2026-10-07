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

//! A frame standing inside a `match` arm survives an edit of the *other*
//! arms' patterns.
//!
//! An arm is named by what it matches, not by where it sits, so that an arm
//! added, removed, reordered or re-patterned leaves the sites of the others
//! alone: a body suspended in the `int t` arm is rebuilt there after its
//! sibling arms moved or changed.

use khora_core::ecs::entity::EntityId;
use khora_core::script::PendingBody;
use khora_script::arena::{Persisted, PersistentStore};
use khora_script::bytecode::init_name;
use khora_script::vm::{resume, ResumeTier, Suspension};
use khora_script::{check, compile, lex, parse, Host, Machine, Program, Run, Value};

const SUBJECT: EntityId = EntityId {
    index: 4,
    generation: 1,
};

fn build(source: &str) -> Program {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let checked = check(&parsed.module);
    assert!(
        checked
            .diagnostics
            .iter()
            .all(|d| d.severity != khora_script::diagnostics::Severity::Error),
        "{:?}",
        checked.diagnostics
    );
    let compiled = compile(&parsed.module);
    assert!(
        compiled.diagnostics.is_empty(),
        "{:?}",
        compiled.diagnostics
    );
    compiled.program
}

/// `Guard` with `Attack(int? target)` matching `target` with `arms`.
fn guard(arms: &str) -> Program {
    build(&format!(
        "behavior Guard {{
             int starts = 0;
             int landed = 0;
             async void Attack(int? target) {{
                 starts += 1;
                 match (target) {{
                     {arms}
                 }}
             }}
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

/// `Guard.Attack(5)` of `original` stopped at its await, resumed into
/// `edited`: the tier and `(starts, landed)` once it finished.
fn attack_resumed(original: &Program, edited: &Program) -> (ResumeTier, [Option<i64>; 2]) {
    let layout = original.layout("Guard").expect("a layout");
    let mut host = Host::new()
        .with_fields(PersistentStore::with_slots(layout.slot_count()))
        .for_entity(SUBJECT);
    let mut init = Machine::new(original, &init_name("Guard"), &[]).expect("defaults");
    finish(&mut init, original, &mut host);
    let mut machine = Machine::new(original, "Guard.Attack", &[Value::Int(5)]).expect("Attack");
    assert_eq!(
        machine.run(original, &mut host, u64::MAX),
        Run::Suspended(Suspension::Awaiting),
        "the body stopped at its await"
    );
    host.awaiting = None;
    let frozen = machine
        .freeze(original, PendingBody::Sequence)
        .expect("a machine of its program freezes");
    let site = frozen
        .frames
        .last()
        .map(|f| f.site.clone())
        .unwrap_or_default();
    assert!(
        site.contains("/arm"),
        "the premise: frozen in an arm, `{site}`"
    );

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

const PRESENT_ARM: &str = "int t => { await 1.0s; landed = t; }";

/// **Arms reordered.** The `null` arm moves above the `int t` arm the body is
/// suspended in; that arm says the same thing, so its site is the same and
/// the frame is rebuilt there — the statement before the `match` is not run
/// again.
#[test]
fn a_frame_inside_a_match_arm_rebuilds_after_the_arms_are_reordered() {
    let original = guard(&format!("{PRESENT_ARM} null => {{ landed = -1; }}"));
    let edited = guard(&format!("null => {{ landed = -1; }} {PRESENT_ARM}"));

    assert_eq!(
        attack_resumed(&original, &edited),
        (ResumeTier::Rebuilt, [Some(1), Some(5)]),
        "(tier, [starts, landed])"
    );
}

/// **Another arm's pattern edited.** `_` becomes `null` — the same cases
/// covered, the suspended arm untouched — and the frame is rebuilt in it.
#[test]
fn a_frame_inside_a_match_arm_rebuilds_after_another_arms_pattern_changes() {
    let original = guard(&format!("{PRESENT_ARM} _ => {{ landed = -1; }}"));
    let edited = guard(&format!("{PRESENT_ARM} null => {{ landed = -1; }}"));

    assert_eq!(
        attack_resumed(&original, &edited),
        (ResumeTier::Rebuilt, [Some(1), Some(5)]),
        "(tier, [starts, landed])"
    );
}
