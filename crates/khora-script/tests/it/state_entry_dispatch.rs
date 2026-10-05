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

//! `Behavior.__enter` is the engine's way into a state, not a member.
//!
//! The compiler emits it beside the behavior's members, under the same
//! `Behavior.name` scheme dispatch resolves events and lifecycle members by.
//! An event a script raises must not reach it: only `become` changes a state
//! from inside a game.

use khora_core::ecs::entity::EntityId;
use khora_script::arena::PersistentStore;
use khora_script::bytecode::init_name;
use khora_script::dispatch::{current_state, deliver, handles, resolve_member};
use khora_script::{check, compile, lex, parse, Host, Machine, Program, Run};

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

fn run_to_end(mut machine: Machine, program: &Program, host: &mut Host) {
    assert_eq!(
        machine.run(program, host, u64::MAX),
        Run::Completed,
        "ran to completion"
    );
}

/// A `Patrol` guard raises `"__enter"` on itself with `1`. Nothing handles an
/// event of that name, so it is not delivered and the guard stays in `Patrol`.
///
/// Observed: `handles(.., "__enter")` is true, the event is delivered to the
/// compiler's `Guard.__enter`, and the guard is in `Chase` — a state change no
/// `become` made.
#[test]
fn an_event_named_like_the_state_entry_is_not_delivered_to_it() {
    let program = build(
        r#"behavior Guard {
               state Patrol { void Probe() { Raise(this, "__enter", 1); } }
               state Chase { }
           }"#,
    );
    let layout = program.layout("Guard").expect("a layout");
    let mut host = Host::new()
        .with_fields(PersistentStore::with_slots(layout.slot_count()))
        .for_entity(SUBJECT);
    let init = Machine::new(&program, &init_name("Guard"), &[]).expect("an initialiser");
    run_to_end(init, &program, &mut host);
    let probe = resolve_member(&program, "Guard", "Probe", &host).expect("`Probe`");
    let machine = Machine::new(&program, &probe, &[]).expect("the member");
    run_to_end(machine, &program, &mut host);

    let events = host.take_events();
    let raised = events.as_slice().first().expect("one event raised").clone();
    let delivered = deliver(&program, "Guard", &raised, &mut host, u64::MAX, |id| {
        id == SUBJECT
    });

    assert_eq!(
        current_state(&program, "Guard", &host),
        Some("Patrol"),
        "an event changed the state"
    );
    assert!(delivered.is_err(), "delivered to the state entry");
    assert!(
        !handles(&program, "Guard", "__enter"),
        "the state entry counts as a handler"
    );
}
