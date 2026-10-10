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

//! A frame rebuilt at its site in edited code holds what the code there reads.
//!
//! Wherever a run was cut — any fuel slice, any frame depth — a body resumed
//! into an edit that kept its site must finish as the edited code would have
//! from that point: the locals it reads are its own, and a value a call just
//! returned is still there for the rest of the statement.

use khora_core::script::{FrozenMachine, PendingBody};
use khora_script::arena::PersistentStore;
use khora_script::bytecode::init_name;
use khora_script::vm::{resume, ResumeTier, Suspension};
use khora_script::{check, compile, lex, parse, Host, Machine, Program, Run, Value};

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

/// Runs `machine` to its end in one go.
fn finish(machine: &mut Machine, program: &Program, host: &mut Host) -> Value {
    for _ in 0..1000 {
        match machine.run(program, host, u64::MAX) {
            Run::Completed => return machine.result(),
            Run::Suspended(_) => host.awaiting = None,
            Run::Faulted(fault) => panic!("faulted: {fault:?}"),
        }
    }
    panic!("still running after 1000 resumes");
}

/// Every cut a run of `F` takes for each slice from one up, frozen.
fn every_cut(program: &Program) -> Vec<(u64, FrozenMachine)> {
    let mut whole = Machine::new(program, "F", &[]).expect("F exists");
    let (_, cost) = whole.run_counting(program, &mut Host::new(), u64::MAX);
    let mut cuts = Vec::new();
    for slice in 1..=cost {
        let mut machine = Machine::new(program, "F", &[]).expect("F exists");
        let mut host = Host::new();
        loop {
            match machine.run(program, &mut host, slice) {
                Run::Completed => break,
                Run::Suspended(Suspension::OutOfFuel) => cuts.push((
                    slice,
                    machine
                        .freeze(program, PendingBody::Update)
                        .expect("a machine of its program freezes"),
                )),
                other => panic!("slice {slice}: {other:?}"),
            }
        }
    }
    cuts
}

/// **The value a call just returned survives a rebuild.** A run cut right
/// after `Seven` returned stands at the call's return site, with the result
/// in the register the rest of the statement reads. Resumed into an edit
/// further down, the frame is rebuilt there — and `got` must be the 7 the
/// call produced, not a blank register.
#[test]
fn a_frame_rebuilt_just_past_a_call_keeps_the_calls_result() {
    let original = build(
        "fn int Seven() { return 7; }
         fn int F() {
             int got = Seven();
             int doubled = got * 2;
             return doubled;
         }",
    );
    let edited = build(
        "fn int Seven() { return 7; }
         fn int F() {
             int got = Seven();
             int doubled = got * 2;
             int unused = 0;
             return doubled;
         }",
    );

    let mut checked = 0;
    for (slice, frozen) in every_cut(&original) {
        let innermost = frozen.frames.last().expect("a frame");
        if !innermost.site.contains(":call.") || frozen.frames.len() != 1 {
            continue;
        }
        checked += 1;
        let (mut machine, tier) = resume(&frozen, original.fingerprint(), &edited)
            .unwrap_or_else(|abandoned| panic!("slice {slice}: abandoned {abandoned:?}"));
        assert_eq!(tier, ResumeTier::Rebuilt, "slice {slice}");
        let mut host = Host::new();
        let mut result = None;
        for _ in 0..10 {
            match machine.run(&edited, &mut host, u64::MAX) {
                Run::Completed => {
                    result = Some(machine.result());
                    break;
                }
                Run::Suspended(_) => {}
                Run::Faulted(fault) => {
                    panic!("slice {slice}: the rebuilt frame faulted: {fault:?}")
                }
            }
        }
        assert_eq!(
            result,
            Some(Value::Int(14)),
            "slice {slice}: cut at `{}`, the call's 7 was lost",
            innermost.site
        );
    }
    assert!(checked > 0, "some slice cuts just past the call");
}

/// **A shadowing local keeps its own value.** At the `await`, two locals are
/// named `x`: the outer 1 and the inner 2, which the rest of the block reads.
/// The edit drops the outer one; the inner `x` the frame is rebuilt with must
/// be the inner value it held, not the outer one that happened to come first.
#[test]
fn a_rebuilt_frame_pairs_a_shadowing_local_with_its_own_value() {
    let behavior = |body: &str| {
        build(&format!(
            "behavior Guard {{
                 int landed = 0;
                 async void Hit() {{ {body} }}
             }}"
        ))
    };
    let original = behavior(
        "int x = 1;
         if (true) {
             int x = 2;
             await 1.0s;
             landed = x;
         }",
    );
    let edited = behavior(
        "if (true) {
             int x = 2;
             await 1.0s;
             landed = x;
         }",
    );

    let layout = original.layout("Guard").expect("a layout");
    let mut host = Host::new().with_fields(PersistentStore::with_slots(layout.slot_count()));
    let mut init = Machine::new(&original, &init_name("Guard"), &[]).expect("defaults");
    finish(&mut init, &original, &mut host);
    let mut machine = Machine::new(&original, "Guard.Hit", &[]).expect("Hit");
    assert_eq!(
        machine.run(&original, &mut host, u64::MAX),
        Run::Suspended(Suspension::Awaiting)
    );
    host.awaiting = None;
    let frozen = machine
        .freeze(&original, PendingBody::Sequence)
        .expect("freezes");

    let (mut machine, tier) =
        resume(&frozen, original.fingerprint(), &edited).expect("the site is still there");
    assert_eq!(tier, ResumeTier::Rebuilt);
    let edited_layout = edited.layout("Guard").expect("a layout");
    let mut host = Host::new().with_fields(PersistentStore::with_slots(edited_layout.slot_count()));
    let mut init = Machine::new(&edited, &init_name("Guard"), &[]).expect("defaults");
    finish(&mut init, &edited, &mut host);
    finish(&mut machine, &edited, &mut host);

    let slot = edited_layout.slot_of("landed").expect("landed");
    assert_eq!(
        host.fields.get(slot),
        Some(&khora_script::arena::Persisted::Scalar(Value::Int(2))),
        "the inner `x` the block reads held 2"
    );
}

/// **Any cut, rebuilt, finishes as the edited code would.** For every fuel
/// slice, every cut of a body with loops, calls with arguments, an `else if`
/// and a nested call is resumed into an edit of its last statement. Pure
/// code: whatever tier it lands on, the result is the edited program's.
#[test]
fn every_cut_resumed_into_edited_code_finishes_with_the_edited_result() {
    let source = |tail: &str| {
        format!(
            "fn int Sum(int a, int b) {{ int s = a + b; return s; }}
             fn int F() {{
                 int total = 0;
                 for (int i = 0; i < 3; i = i + 1) {{
                     total = total + Sum(i, Sum(1, i)) * 2 - Sum(1, 1);
                 }}
                 int k = 0;
                 while (k < 2) {{
                     k = k + 1;
                     total = total + k;
                 }}
                 if (total < 0) {{
                     total = 0;
                 }} else if (total > 1) {{
                     total = Sum(total, 1);
                 }}
                 {tail}
             }}"
        )
    };
    let original = build(&source("return total;"));
    let edited = build(&source("int bonus = 1000;\n return total + bonus;"));
    let mut whole = Machine::new(&edited, "F", &[]).expect("F");
    let expected = finish(&mut whole, &edited, &mut Host::new());

    let mut rebuilt = 0;
    for (slice, frozen) in every_cut(&original) {
        let (mut machine, tier) = resume(&frozen, original.fingerprint(), &edited)
            .unwrap_or_else(|abandoned| panic!("slice {slice}: abandoned {abandoned:?}"));
        if tier == ResumeTier::Rebuilt {
            rebuilt += 1;
        }
        let mut host = Host::new();
        let mut result = None;
        for _ in 0..10 {
            match machine.run(&edited, &mut host, u64::MAX) {
                Run::Completed => {
                    result = Some(machine.result());
                    break;
                }
                Run::Suspended(_) => {}
                Run::Faulted(fault) => panic!(
                    "slice {slice}, frozen at {:?}: {tier:?} faulted: {fault:?}",
                    frozen.frames.iter().map(|f| &f.site).collect::<Vec<_>>()
                ),
            }
        }
        assert_eq!(
            result,
            Some(expected),
            "slice {slice}, frozen at {:?}, resumed {tier:?}",
            frozen.frames.iter().map(|f| &f.site).collect::<Vec<_>>()
        );
    }
    assert!(rebuilt > 0, "some cut was rebuilt");
}

/// **A malformed save is refused, never a panic.** A frozen machine is input
/// nobody sized: frames standing at the wrong kind of site, registers far past
/// any window, a site in the wrong function. Each is resumed into edited code
/// and, if taken back, run a little.
#[test]
fn a_malformed_frozen_machine_resumes_or_is_refused_without_panicking() {
    let source = |tail: &str| {
        format!(
            "behavior Guard {{
             async int Wind(int power) {{ int got = power; await 1.0s; return got * 2; }}
             async int F() {{
                 int a = 3;
                 int r = Wind(a) + a;
                 {tail}
             }}
             }}"
        )
    };
    let original = build(&source("return r;"));
    let edited = build(&source("int z = 1;\n return r + z;"));
    let mut machine = Machine::new(&original, "Guard.F", &[]).expect("F");
    assert_eq!(
        machine.run(&original, &mut Host::new(), u64::MAX),
        Run::Suspended(Suspension::Awaiting)
    );
    let good = machine
        .freeze(&original, PendingBody::Update)
        .expect("freezes");
    assert_eq!(good.frames.len(), 2);
    let caller_site = good.frames[0].site.clone();
    let await_site = good.frames[1].site.clone();

    type Mutation = (&'static str, Box<dyn Fn(&mut FrozenMachine)>);
    let mutations: Vec<Mutation> = vec![
        ("a huge base", Box::new(|m| m.frames[1].base = u64::MAX)),
        (
            "a local past every register",
            Box::new(|m| {
                for local in &mut m.frames[1].locals {
                    local.register = u64::MAX;
                }
            }),
        ),
        (
            "a temporary past every register",
            Box::new(|m| m.frames[1].temporaries = vec![u64::MAX; m.frames[1].temporaries.len()]),
        ),
        (
            "the caller at the callee's await site",
            Box::new(move |m| m.frames[0].site = await_site.clone()),
        ),
        (
            "the callee at the caller's return site",
            Box::new(move |m| m.frames[1].site = caller_site.clone()),
        ),
        ("the frames swapped", Box::new(|m| m.frames.swap(0, 1))),
        ("a huge counter", Box::new(|m| m.program_counter = u64::MAX)),
        ("a huge result", Box::new(|m| m.frames[1].result = u64::MAX)),
        ("no registers", Box::new(|m| m.registers.clear())),
        (
            "a thousand frames",
            Box::new(|m| {
                let callee = m.frames[1].clone();
                m.frames.pop();
                for _ in 0..1000 {
                    m.frames.push(m.frames[0].clone());
                }
                m.frames.push(callee);
            }),
        ),
    ];

    for (what, mutate) in mutations {
        for (into, program) in [("its own program", &original), ("edited code", &edited)] {
            let mut frozen = good.clone();
            mutate(&mut frozen);
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if let Ok((mut machine, _)) = resume(&frozen, original.fingerprint(), program) {
                    let mut host = Host::new();
                    for _ in 0..4 {
                        if !matches!(machine.run(program, &mut host, 10_000), Run::Suspended(_)) {
                            break;
                        }
                        host.awaiting = None;
                    }
                }
            }));
            assert!(outcome.is_ok(), "{what}, resumed into {into}: panicked");
        }
    }
}

/// **A frame whose code did not change is taken as it stands.** The `await`
/// is inside `Wind`, whose only edit is a local's name — the same bytecode,
/// so its frame is still exactly valid. Its caller was edited after the call.
/// The stack is rebuilt — the caller at its site, the callee as it was —
/// rather than run again from the top, which would count `starts` twice.
#[test]
fn an_unchanged_callee_does_not_force_its_edited_caller_to_restart() {
    let behavior = |wind_local: &str, after: &str| {
        build(&format!(
            "behavior Guard {{
                 int starts = 0;
                 int landed = 0;
                 async int Wind() {{
                     int {wind_local} = 5;
                     await 1.0s;
                     return {wind_local} * 2;
                 }}
                 async void Strike() {{
                     starts += 1;
                     int got = Wind();
                     landed = got{after};
                 }}
             }}"
        ))
    };
    let original = behavior("power", "");
    let edited = behavior("strength", " + 100");
    assert_eq!(
        original.function("Guard.Wind").map(|f| f.fingerprint),
        edited.function("Guard.Wind").map(|f| f.fingerprint),
        "the premise: renaming a local leaves `Wind`'s code as it was"
    );

    let layout = original.layout("Guard").expect("a layout");
    let mut host = Host::new().with_fields(PersistentStore::with_slots(layout.slot_count()));
    let mut init = Machine::new(&original, &init_name("Guard"), &[]).expect("defaults");
    finish(&mut init, &original, &mut host);
    let mut machine = Machine::new(&original, "Guard.Strike", &[]).expect("Strike");
    assert_eq!(
        machine.run(&original, &mut host, u64::MAX),
        Run::Suspended(Suspension::Awaiting)
    );
    host.awaiting = None;
    let frozen = machine
        .freeze(&original, PendingBody::Sequence)
        .expect("freezes");

    let (mut machine, tier) =
        resume(&frozen, original.fingerprint(), &edited).expect("Strike is still there");
    finish(&mut machine, &edited, &mut host);
    let read = |name: &str| {
        host.fields
            .get(layout.slot_of(name).expect("a field"))
            .cloned()
    };

    assert_eq!(
        (tier, read("starts"), read("landed")),
        (
            ResumeTier::Rebuilt,
            Some(khora_script::arena::Persisted::Scalar(Value::Int(1))),
            Some(khora_script::arena::Persisted::Scalar(Value::Int(110))),
        ),
        "rebuilt, `starts` counted once, the edited caller ran"
    );
}

/// **A caller still waiting on its call does not read its result register.**
/// `Strike` stands at its call to `Wind`, which is still running: the register
/// the result will land in lies inside `Wind`'s window and holds one of
/// `Wind`'s locals — `"wind-up"`. The edit drops that local, and its literal,
/// from `Wind`, and edits `Strike` after the call. Nothing will ever read
/// that register's old value — the return overwrites it — so the stack is
/// rebuilt rather than run again from the top, which would count `starts`
/// twice.
#[test]
fn a_caller_waiting_on_its_call_is_rebuilt_whatever_its_result_register_held() {
    let behavior = |wind_local: &str, after: &str| {
        build(&format!(
            "behavior Guard {{
                 int starts = 0;
                 int landed = 0;
                 async int Wind() {{
                     string keep = \"keep\";
                     {wind_local}
                     await 1.0s;
                     return 2;
                 }}
                 async void Strike() {{
                     starts += 1;
                     int got = Wind();
                     landed = got{after};
                 }}
             }}"
        ))
    };
    let original = behavior("string spare = \"wind-up\";", "");
    let edited = behavior("", " + 100");

    let layout = original.layout("Guard").expect("a layout");
    let mut host = Host::new().with_fields(PersistentStore::with_slots(layout.slot_count()));
    let mut init = Machine::new(&original, &init_name("Guard"), &[]).expect("defaults");
    finish(&mut init, &original, &mut host);
    let mut machine = Machine::new(&original, "Guard.Strike", &[]).expect("Strike");
    assert_eq!(
        machine.run(&original, &mut host, u64::MAX),
        Run::Suspended(Suspension::Awaiting)
    );
    host.awaiting = None;
    let frozen = machine
        .freeze(&original, PendingBody::Sequence)
        .expect("freezes");
    assert_eq!(frozen.frames.len(), 2, "the premise: stopped inside `Wind`");

    let (mut machine, tier) =
        resume(&frozen, original.fingerprint(), &edited).expect("Strike is still there");
    finish(&mut machine, &edited, &mut host);
    let read = |name: &str| {
        host.fields
            .get(layout.slot_of(name).expect("a field"))
            .cloned()
    };

    assert_eq!(
        (tier, read("starts"), read("landed")),
        (
            ResumeTier::Rebuilt,
            Some(khora_script::arena::Persisted::Scalar(Value::Int(1))),
            Some(khora_script::arena::Persisted::Scalar(Value::Int(102))),
        ),
        "rebuilt at both sites, `starts` counted once"
    );
}

/// **An outer local is not handed the value of a local that shadowed it.** At
/// the `await`, the outer `x` holds 1 and the inner `x` holds 2. The edit
/// removes the inner declaration, so the block — and the statement after it —
/// read the outer `x`, which no version of the code ever set to 2. However
/// the body comes back, the outer `x` must still be 1.
#[test]
fn an_outer_local_keeps_its_value_when_the_local_shadowing_it_is_removed() {
    let behavior = |inner: &str| {
        build(&format!(
            "behavior Guard {{
                 int landed = 0;
                 int outer = 0;
                 async void Hit() {{
                     int x = 1;
                     if (true) {{
                         {inner}
                         await 1.0s;
                         landed = x;
                     }}
                     outer = x;
                 }}
             }}"
        ))
    };
    let original = behavior("int x = 2;");
    let edited = behavior("");

    let layout = original.layout("Guard").expect("a layout");
    let mut host = Host::new().with_fields(PersistentStore::with_slots(layout.slot_count()));
    let mut init = Machine::new(&original, &init_name("Guard"), &[]).expect("defaults");
    finish(&mut init, &original, &mut host);
    let mut machine = Machine::new(&original, "Guard.Hit", &[]).expect("Hit");
    assert_eq!(
        machine.run(&original, &mut host, u64::MAX),
        Run::Suspended(Suspension::Awaiting)
    );
    host.awaiting = None;
    let frozen = machine
        .freeze(&original, PendingBody::Sequence)
        .expect("freezes");

    let (mut machine, tier) =
        resume(&frozen, original.fingerprint(), &edited).expect("Hit is still there");
    finish(&mut machine, &edited, &mut host);

    let slot = layout.slot_of("outer").expect("outer");
    assert_eq!(
        host.fields.get(slot),
        Some(&khora_script::arena::Persisted::Scalar(Value::Int(1))),
        "resumed {tier:?}: the outer `x` was 1 and nothing in either version set it to 2"
    );
}
