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

//! A rebuilt frame carries a local's value exactly when the local can hold it.
//!
//! An edit after an `await` rebuilds the frame at the same site, each local
//! carried by name and type; the statements before the `await` do not run
//! again. A value the new local cannot hold must not be carried — and a value
//! it *can* hold must not force a restart, which runs those statements twice.

use khora_core::ecs::entity::EntityId;
use khora_core::script::PendingBody;
use khora_script::arena::{Persisted, PersistentStore};
use khora_script::bytecode::init_name;
use khora_script::vm::{resume, Fault, ResumeTier, Suspension};
use khora_script::{check, compile, lex, parse, Host, Machine, Program, Run, Value};

const SUBJECT: EntityId = EntityId {
    index: 1,
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

/// `Guard` with `attack` as its member `Attack`.
fn guard(attack: &str) -> Program {
    build(&format!(
        "behavior Guard {{
             int starts = 0;
             int landed = 0;
             int extra = 0;
             {attack}
         }}"
    ))
}

/// Runs `machine` to the end, every `await` treated as elapsed.
fn finish(mut machine: Machine, program: &Program, host: &mut Host) -> Result<Value, Fault> {
    for _ in 0..1000 {
        match machine.run(program, host, u64::MAX) {
            Run::Completed => return Ok(machine.result()),
            Run::Suspended(_) => host.awaiting = None,
            Run::Faulted(fault) => return Err(fault),
        }
    }
    panic!("still running after 1000 resumes");
}

/// What a resume did: its tier, how the body ended, and `(starts, landed,
/// extra)` afterwards.
#[derive(Debug)]
struct Resumed {
    tier: ResumeTier,
    outcome: Result<Value, Fault>,
    fields: [Option<i64>; 3],
}

/// Runs `Attack` of `original` to its `await`, freezes it, and resumes it into
/// `edited` on the same instance.
fn resumed(original: &str, edited: &str) -> Resumed {
    let original = guard(original);
    let edited = guard(edited);

    let layout = original.layout("Guard").expect("a layout");
    let mut host = Host::new()
        .with_fields(PersistentStore::with_slots(layout.slot_count()))
        .for_entity(SUBJECT);
    let init = Machine::new(&original, &init_name("Guard"), &[]).expect("an initialiser");
    finish(init, &original, &mut host).expect("the defaults run");

    let mut machine = Machine::new(&original, "Guard.Attack", &[]).expect("the member");
    assert_eq!(
        machine.run(&original, &mut host, u64::MAX),
        Run::Suspended(Suspension::Awaiting),
        "the body stopped at its await"
    );
    let frozen = machine
        .freeze(&original, PendingBody::Sequence)
        .expect("a machine of this program freezes");

    let (machine, tier) = resume(&frozen, original.fingerprint(), &edited)
        .unwrap_or_else(|abandoned| panic!("abandoned: {abandoned:?}"));
    let mut host = Host::new()
        .with_fields(host.fields.clone())
        .for_entity(SUBJECT);
    let outcome = finish(machine, &edited, &mut host);

    let layout = edited.layout("Guard").expect("a layout");
    let field = |name: &str| match host.fields.get(layout.slot_of(name)?) {
        Some(Persisted::Scalar(Value::Int(value))) => Some(*value),
        _ => None,
    };
    Resumed {
        tier,
        outcome,
        fields: [field("starts"), field("landed"), field("extra")],
    }
}

/// `var b = maybe` holds the `null` of an `int?` — a value it can hold. An
/// edit after the `await` that leaves `b` as it was rebuilds the frame; the
/// `starts += 1` before the `await` runs once.
///
/// Observed: the site records `b` as `int` and the rebuild guard refuses its
/// `null`, so the body restarts — `Restarted`, `starts == 2`.
#[test]
fn a_var_holding_null_is_carried_into_the_same_var() {
    let resumed = resumed(
        "async void Attack() {
             starts += 1;
             int? maybe = null;
             var b = maybe;
             await 1.0s;
             landed = b ?? 41;
         }",
        "async void Attack() {
             starts += 1;
             int? maybe = null;
             var b = maybe;
             await 1.0s;
             extra += 1;
             landed = b ?? 41;
         }",
    );

    assert_eq!(resumed.tier, ResumeTier::Rebuilt, "{resumed:?}");
    assert_eq!(
        resumed.fields,
        [Some(1), Some(41), Some(1)],
        "(starts, landed, extra): {resumed:?}"
    );
}

/// `int count;` declared without a value and not yet assigned at the `await`
/// is an unassigned `int`. An edit after the `await` rebuilds the frame; the
/// statements before it run once.
///
/// Observed: the local holds `Unit`, the rebuild guard refuses it as an
/// `int`, and the body restarts — `Restarted`, `starts == 2`.
#[test]
fn a_local_declared_without_a_value_does_not_force_a_restart() {
    let resumed = resumed(
        "async void Attack() {
             starts += 1;
             int count;
             await 1.0s;
             count = 3;
             landed = count;
         }",
        "async void Attack() {
             starts += 1;
             int count;
             await 1.0s;
             extra += 1;
             count = 3;
             landed = count;
         }",
    );

    assert_eq!(resumed.tier, ResumeTier::Rebuilt, "{resumed:?}");
    assert_eq!(
        resumed.fields,
        [Some(1), Some(3), Some(1)],
        "(starts, landed, extra): {resumed:?}"
    );
}

/// `var hit = this` is an `Entity`; edited to `var hit = true` it is a `bool`.
/// Both are recorded as `var`, which the rebuild guard takes on trust, so the
/// entity must not reach the `bool` local: the body restarts, or at least
/// never hands an entity to `if`.
///
/// Observed: `Rebuilt`, then `if (hit)` faults with `TypeMismatch { expected:
/// "bool", found: "Entity" }`.
#[test]
fn a_var_whose_type_changed_is_not_carried() {
    let resumed = resumed(
        "async void Attack() {
             starts += 1;
             var hit = this;
             await 1.0s;
             landed = 1;
         }",
        "async void Attack() {
             starts += 1;
             var hit = true;
             await 1.0s;
             if (hit) { landed = 2; }
         }",
    );

    assert!(resumed.outcome.is_ok(), "{resumed:?}");
    assert_eq!(resumed.fields[1], Some(2), "landed: {resumed:?}");
}
