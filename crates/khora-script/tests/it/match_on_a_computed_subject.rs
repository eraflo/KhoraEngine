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

//! A frame standing in a `null` or `_` arm of a `match` whose subject is
//! computed — a field read, a call — survives an edit of the other arms.
//!
//! The subject is evaluated into a temporary. An arm that binds nothing may
//! leave it live while its body runs, so where that arm sits must not decide
//! what an await in it records: the arm's name does not depend on its
//! position, and neither may what a rebuilt frame needs from it.

use khora_core::ecs::entity::EntityId;
use khora_core::script::PendingBody;
use khora_script::arena::{Persisted, PersistentStore};
use khora_script::bytecode::init_name;
use khora_script::vm::{resume, ResumeTier, Suspension};
use khora_script::{check, compile, lex, parse, Host, Machine, Program, Run, Value};

const SUBJECT: EntityId = EntityId {
    index: 9,
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

/// `Guard` with `Attack()` matching its field `best` — never set, so `null` —
/// with `arms`.
fn guard(arms: &str) -> Program {
    build(&format!(
        "behavior Guard {{
             int starts = 0;
             int landed = 0;
             int? best;
             async void Attack() {{
                 starts += 1;
                 match (best) {{
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

/// `Guard.Attack()` of `original` stopped at its await, resumed into `edited`:
/// the tier and `(starts, landed)` once it finished.
fn attack_resumed(original: &Program, edited: &Program) -> (ResumeTier, [Option<i64>; 2]) {
    let layout = original.layout("Guard").expect("a layout");
    let mut host = Host::new()
        .with_fields(PersistentStore::with_slots(layout.slot_count()))
        .for_entity(SUBJECT);
    let mut init = Machine::new(original, &init_name("Guard"), &[]).expect("defaults");
    finish(&mut init, original, &mut host);
    let mut machine = Machine::new(original, "Guard.Attack", &[]).expect("Attack");
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

const NULL_ARM: &str = "null => { await 1.0s; landed = -1; }";

/// **The arms reordered, the suspended one moving down.** The body stopped in
/// the first arm, `null`, over the field read `best`; the edit moves `int t`
/// above it. The `null` arm says what it said, its site keeps its name, and
/// the frame is rebuilt there — `starts += 1` is not run a second time.
#[test]
fn a_frame_in_a_null_arm_over_a_computed_subject_rebuilds_after_the_arms_are_reordered() {
    let original = guard(&format!("{NULL_ARM} int t => {{ landed = t; }}"));
    let edited = guard(&format!("int t => {{ landed = t; }} {NULL_ARM}"));

    assert_eq!(
        attack_resumed(&original, &edited),
        (ResumeTier::Rebuilt, [Some(1), Some(-1)]),
        "(tier, [starts, landed])"
    );
}
