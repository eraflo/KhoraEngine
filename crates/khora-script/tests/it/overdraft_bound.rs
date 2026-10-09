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

//! What a run spends past its fuel is bounded by `Program::max_overdraft`.
//!
//! A run that has used its slice goes on only to the next place it may stop
//! at, and the compiler records the costliest such stretch as the program's
//! overdraft bound — what the DCC relies on to keep a frame. A sized operation
//! the run starts at is the one stated exception (it runs alone in its slice);
//! every other operation, however large, either fits in the bound or is a
//! place the run stops before.

use khora_core::ecs::entity::EntityId;
use khora_script::arena::PersistentStore;
use khora_script::bytecode::init_name;
use khora_script::dispatch::resolve_member;
use khora_script::vm::{Fault, Machine, Program, Run, Value};
use khora_script::{check, compile, lex, parse, Host};

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

fn finish(machine: &mut Machine, program: &Program, host: &mut Host) -> Result<Value, Fault> {
    for _ in 0..10_000 {
        match machine.run(program, host, u64::MAX) {
            Run::Completed => return Ok(machine.result()),
            Run::Suspended(_) => {
                host.awaiting = None;
                host.end_frame();
            }
            Run::Faulted(fault) => return Err(fault),
        }
    }
    panic!("no end");
}

fn instance(program: &Program) -> Host {
    let layout = program.layout("Guard").expect("the guard's layout");
    let mut host = Host::new()
        .with_fields(PersistentStore::with_slots(layout.slot_count()))
        .for_entity(SUBJECT);
    let mut init = Machine::new(program, &init_name("Guard"), &[]).expect("a field initialiser");
    finish(&mut init, program, &mut host).expect("the initialiser runs");
    host.end_frame();
    host
}

/// **No run spends more than its fuel plus the overdraft bound, unless it
/// started at a sized operation.** A guard holds a ten-by-hundred grid built
/// from a small literal, so the code is short and the data is large. `Set`
/// writes one cell through the field (`grid[0][0] = 1`); `Go` enters a state
/// with the grid as its argument. Each body begins with a one-unit statement,
/// so no sized operation is the first thing its first run does. Run once at
/// every fuel from 1 to 3 000, the first run never spends more than
/// `fuel + max_overdraft`. A store or a `become` that is charged its size but
/// never stopped before — and counted as one unit by the bound — breaks this
/// by about a thousand.
#[test]
fn no_run_spends_past_its_fuel_more_than_the_overdraft_bound() {
    let program = build(&format!(
        "behavior Guard {{
             int[] row = [{}];
             int[][] grid = [row, row, row, row, row, row, row, row, row, row];
             void Set() {{ int k = 0; grid[0][0] = 1; }}
             state Idle {{
                 void Go() {{ int k = 0; become Wide(grid); }}
             }}
             state Wide(int[][] g) {{ int First() {{ return g.Length; }} }}
         }}",
        ints(100)
    ));
    let bound = program.max_overdraft();
    let mut wrong = Vec::new();
    for member in ["Set", "Go"] {
        let mut worst: Option<(u64, u64)> = None;
        for fuel in 1..=3_000u64 {
            let mut host = instance(&program);
            let function = resolve_member(&program, "Guard", member, &host)
                .unwrap_or_else(|| panic!("the guard has a member `{member}`"));
            let mut machine = Machine::new(&program, &function, &[]).expect("the member exists");
            let (run, spent) = machine.run_counting(&program, &mut host, fuel);
            assert!(
                !matches!(run, Run::Faulted(_)),
                "`{member}` faulted at fuel {fuel}: {run:?}"
            );
            let past = spent.saturating_sub(fuel);
            if past > bound && worst.is_none_or(|(_, seen)| past > seen) {
                worst = Some((fuel, past));
            }
        }
        if let Some((fuel, past)) = worst {
            wrong.push(format!(
                "`{member}`: at fuel {fuel} the run spent {past} past its fuel; the bound is {bound}"
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
