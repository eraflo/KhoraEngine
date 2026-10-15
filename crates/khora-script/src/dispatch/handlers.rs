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

//! Which function handles an event, per program.
//!
//! Built once from the program — the events it declares and the functions its
//! handlers compiled to — the first time a delivery asks, and never per entity:
//! it says what the *code* handles, and the entity's state picks among it at
//! delivery. Only an event the program declares is in it, so a method or a
//! lifecycle hook is never reached by an event of its name.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use crate::vm::Program;

/// The handlers of one event in one behavior, as indices into the
/// program's functions.
#[derive(Debug, Clone, Default)]
pub struct Handlers {
    /// The behavior's own, outside any state.
    pub plain: Option<usize>,
    /// Each state's, by state.
    pub states: HashMap<String, usize>,
}

/// Event → behavior → its handlers. A cache of what the program says: equal
/// for any two programs, rebuilt after a program is read back.
#[derive(Default, Clone)]
pub struct HandlerTable(OnceLock<HashMap<String, HashMap<String, Handlers>>>);

impl PartialEq for HandlerTable {
    // Derived from the program it sits in, so it never tells two programs
    // apart: one asked already and one not are the same program.
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl std::fmt::Debug for HandlerTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HandlerTable")
    }
}

impl HandlerTable {
    /// The handlers of `event` in `behavior`, if it handles it.
    pub fn of<'p>(
        &'p self,
        program: &'p Program,
        behavior: &str,
        event: &str,
    ) -> Option<&'p Handlers> {
        self.0
            .get_or_init(|| build(program))
            .get(event)?
            .get(behavior)
    }
}

fn build(program: &Program) -> HashMap<String, HashMap<String, Handlers>> {
    let events: HashSet<&str> = program
        .events
        .iter()
        .map(|event| event.name.as_str())
        .collect();
    let mut table: HashMap<String, HashMap<String, Handlers>> = HashMap::new();
    for (index, function) in program.functions.iter().enumerate() {
        let parts: Vec<&str> = function.name.split('.').collect();
        let (behavior, state, event) = match parts.as_slice() {
            [behavior, event] => (*behavior, None, *event),
            [behavior, state, event] => (*behavior, Some(*state), *event),
            _ => continue,
        };
        // A method may not take an event's name, so `Guard.Damaged` is the
        // handler — and a state's `Guard.Patrol.Damaged` only where `Patrol`
        // is one of its states.
        if !events.contains(event) {
            continue;
        }
        let handlers = table
            .entry(event.to_owned())
            .or_default()
            .entry(behavior.to_owned())
            .or_default();
        match state {
            None => handlers.plain = Some(index),
            Some(state) => {
                let is_state = program
                    .layout(behavior)
                    .is_some_and(|layout| layout.states.iter().any(|known| known.name == state));
                if is_state {
                    handlers.states.insert(state.to_owned(), index);
                }
            }
        }
    }
    table
}
