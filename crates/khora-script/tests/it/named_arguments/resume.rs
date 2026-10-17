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

//! A frame stopped inside a labelled call, resumed after an edit that
//! reorders the callee's parameters.
//!
//! The arguments already evaluated are held in the order they were written,
//! which the edit does not touch: whether the frame is kept, rebuilt at its
//! site or restarted, each value reaches the parameter it was named for —
//! never its neighbour's.

use khora_core::script::{FrozenMachine, PendingBody};
use khora_script::vm::{resume, ResumeTier, Suspension};
use khora_script::{Host, Machine, Program, Run, Value};

use super::{build, finish, Instance};
use crate::support::fresh_store;

/// Every cut a run of `F` takes for each fuel slice from one up, frozen.
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

/// **Out of fuel, anywhere in the call.** `F` calls `Pair(b: Two(), a:
/// Three())`: `b` is evaluated first, and held while `a` is. Cut at every
/// slice — inside `Two`, between the two arguments, inside `Pair` — and
/// resumed into a `Pair` declared `(int b, int a)`, every run returns
/// `a * 10 + b = 32`. A 23 is the two values swapped.
#[test]
fn a_frame_cut_in_a_labelled_call_resumes_into_reordered_parameters_without_swapping() {
    let source = |parameters: &str| {
        format!(
            "fn int Two() {{ return 2; }}
             fn int Three() {{ return 3; }}
             fn int Pair({parameters}) {{ return a * 10 + b; }}
             fn int F() {{
                 int got = Pair(b: Two(), a: Three());
                 return got;
             }}"
        )
    };
    let original = build(&source("int a, int b"));
    let edited = build(&source("int b, int a"));

    let mut between = 0;
    for (slice, frozen) in every_cut(&original) {
        let sites: Vec<&String> = frozen.frames.iter().map(|frame| &frame.site).collect();
        let (mut machine, tier) = resume(&frozen, original.fingerprint(), &edited)
            .unwrap_or_else(|abandoned| panic!("slice {slice} at {sites:?}: {abandoned:?}"));
        if frozen.frames.len() == 1 && frozen.frames[0].site.contains(":call.Three") {
            // Stopped just after `Three` returned: both arguments evaluated,
            // `Pair` not yet entered.
            between += 1;
            assert_eq!(
                tier,
                ResumeTier::Rebuilt,
                "slice {slice}: the caller's code changed, its site did not"
            );
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
                Run::Faulted(fault) => panic!("slice {slice} at {sites:?}: faulted {fault:?}"),
            }
        }
        assert_eq!(
            result,
            Some(Value::Int(32)),
            "slice {slice}, cut at {sites:?}, resumed {tier:?}"
        );
    }
    assert!(
        between > 0,
        "some slice cuts with both arguments held and the call not made"
    );
}

/// `Guard` whose `Go` calls the `async` `Strike` with its arguments named
/// against the order `parameters` declares them in; `Strike` waits, then
/// writes `damage * 10 + times` to `landed`.
fn striking(parameters: &str) -> Program {
    build(&format!(
        "behavior Guard {{
             int landed = 0;
             int Two() {{ return 2; }}
             async void Strike({parameters}) {{
                 await 1.0s;
                 landed = damage * 10 + times;
             }}
             async void Go() {{ Strike(times: Two(), damage: 3); }}
         }}"
    ))
}

/// **Waiting in the callee.** `Go` is stopped at the `await` inside `Strike`,
/// called with labels, and frozen. Resumed into a `Strike` declared `(int
/// times, int damage)`, it finishes writing `3 * 10 + 2 = 32`.
#[test]
fn a_frame_awaiting_in_a_labelled_call_resumes_into_reordered_parameters_without_swapping() {
    let original = striking("int damage, int times");
    let edited = striking("int times, int damage");

    let mut host = Host::new().with_fields(fresh_store(&original, "Guard"));
    let mut machine = Machine::new(&original, "Guard.Go", &[]).expect("Go");
    assert_eq!(
        machine.run(&original, &mut host, u64::MAX),
        Run::Suspended(Suspension::Awaiting),
        "the premise: stopped at the `await` in `Strike`"
    );
    host.awaiting = None;
    let frozen = machine
        .freeze(&original, PendingBody::Sequence)
        .expect("a machine of its program freezes");
    assert_eq!(frozen.frames.len(), 2, "stopped inside the call");

    let (machine, tier) = resume(&frozen, original.fingerprint(), &edited)
        .unwrap_or_else(|abandoned| panic!("abandoned {abandoned:?}"));
    assert_ne!(tier, ResumeTier::Exact, "the program changed");

    let mut guard = Instance::of(&edited, "Guard");
    finish(machine, &edited, &mut guard.host);
    assert_eq!(
        guard.field("landed"),
        Value::Int(32),
        "`damage` is 3 and `times` is 2, resumed {tier:?}"
    );
}
