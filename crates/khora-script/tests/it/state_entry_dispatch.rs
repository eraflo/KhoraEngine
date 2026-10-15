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

//! Only `become` changes a state from inside a game.
//!
//! What entering a state writes is data in the behavior's layout, not a
//! function compiled beside its members, so an event raised under the name the
//! state entry once had (`__enter`) reaches nothing.

use crate::support::fresh_store;
use khora_core::ecs::entity::EntityId;
use khora_core::script::{ScriptEvent, ScriptValue};
use khora_script::dispatch::{current_state, deliver, handles};
use khora_script::{check, compile, lex, parse, Host, Program};

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

/// A `Patrol` guard is handed an event named `"__enter"` carrying `1` — as
/// the engine channel would hand it one; a script cannot name it. Nothing
/// handles an event of that name, so it is not delivered and the guard stays
/// in `Patrol`.
///
/// Observed while the state entry was a function: `handles(.., "__enter")`
/// was true, the event was delivered to the compiler's `Guard.__enter`, and
/// the guard was in `Chase` — a state change no `become` made.
#[test]
fn an_event_named_like_the_state_entry_is_not_delivered_to_it() {
    let program = build(
        r#"behavior Guard {
               state Patrol { void Probe() { } }
               state Chase { }
           }"#,
    );
    let mut host = Host::new()
        .with_fields(fresh_store(&program, "Guard"))
        .for_entity(SUBJECT);

    let raised = ScriptEvent::new(SUBJECT, "__enter").with(ScriptValue::Int(1));
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
