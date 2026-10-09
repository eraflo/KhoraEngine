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

//! A removal reached again in the same run, after the run started at it.
//!
//! A run that cannot pay for `xs.RemoveAt(i)` on a variable's array stops at
//! the start of the path to the array; the next run starts there and pays for
//! that removal whatever it costs, so every run makes progress. That licence
//! is for the removal the run started at, once — not for every removal the
//! run reaches at the same place, as a loop does on its next turn.

use khora_core::script::PendingBody;
use khora_script::vm::{Machine, Program, Run, Suspension};
use khora_script::{check, compile, lex, parse, Host};

fn build(source: &str) -> Program {
    let lexed = lex(source);
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    let parsed = parse(lexed.tokens);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let checked = check(&parsed.module);
    assert!(!checked.has_errors(), "{:?}", checked.diagnostics);
    let compiled = compile(&parsed.module);
    assert!(!compiled.has_errors(), "{:?}", compiled.diagnostics);
    compiled.program
}

/// `count` integer literals `0, 1, …`, comma-separated.
fn ints(count: usize) -> String {
    (0..count)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The site the innermost frame of `machine` stands at.
fn site(machine: &Machine, program: &Program) -> String {
    machine
        .freeze(program, PendingBody::Sequence)
        .and_then(|frozen| frozen.frames.last().map(|frame| frame.site.clone()))
        .unwrap_or_default()
}

/// **A run that started at a removal stops before the next removal it cannot
/// pay for.** A loop removes the first of a hundred elements three times.
/// Five fuel at a time, the body stops before the first removal; the next
/// run, given 150, starts there and pays the hundred that removal costs. On
/// the loop's next turn the same removal, now 99, is more than the fuel left:
/// the run must stop before it, at the same site, having spent no more than
/// its 150 — not run it on credit because it stands where the run began.
#[test]
fn a_run_resumed_at_a_removal_stops_before_the_next_one_it_cannot_pay_for() {
    let program = build(&format!(
        "fn int F() {{
             int[] a = [{}];
             int n = 0;
             while (n < 3) {{
                 a.RemoveAt(0);
                 n += 1;
             }}
             return a.Length;
         }}",
        ints(100)
    ));
    let mut machine = Machine::new(&program, "F", &[]).expect("the function exists");
    let mut host = Host::new();

    let mut before_removal = false;
    for _ in 0..50 {
        let (run, _) = machine.run_counting(&program, &mut host, 5);
        assert_eq!(
            run,
            Run::Suspended(Suspension::OutOfFuel),
            "the premise: five fuel at a time stops before the loop is done"
        );
        host.end_frame();
        if site(&machine, &program).ends_with(":remove") {
            before_removal = true;
            break;
        }
    }
    assert!(
        before_removal,
        "the premise: a run stops before the first removal"
    );

    let (run, spent) = machine.run_counting(&program, &mut host, 150);
    assert_eq!(
        run,
        Run::Suspended(Suspension::OutOfFuel),
        "the run cannot pay for two removals of a hundred and ninety-nine"
    );
    assert!(
        spent <= 150,
        "the run given 150 spent {spent}: the second removal ran on credit"
    );
    assert!(
        site(&machine, &program).ends_with(":remove"),
        "the run stops before the second removal: `{}`",
        site(&machine, &program)
    );
}
