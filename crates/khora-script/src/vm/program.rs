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

//! A compiled program.
//!
//! Functions are addressed by index rather than by name: resolution happens
//! once, at compile time, so a call is a jump into a table instead of a lookup
//! on every invocation.

use serde::{Deserialize, Serialize};

use super::instruction::Instruction;

/// One compiled function.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Function {
    /// Name, for diagnostics and for the engine to call by.
    pub name: String,
    /// How many parameters it takes. They arrive in registers `0..arity`.
    pub arity: usize,
    /// How many registers its frame needs, parameters included.
    pub registers: usize,
    /// Its instructions.
    pub code: Vec<Instruction>,
}

/// One `state` of a behavior, and the slots its own data occupies.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct StateLayout {
    /// The state's name.
    pub name: String,
    /// Its parameters then its fields, in declaration order.
    ///
    /// Named together because `become Chase(prey)` fills the first and the
    /// state's own `int missed = 0;` fills the rest, and once entered a member
    /// reads both the same way.
    pub slots: Vec<String>,
}

/// Whether a scheduled member repeats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimerKind {
    /// `every 0.5s { … }` — fires again each interval.
    Every,
    /// `after 10s => … ` — fires once, then never.
    After,
}

/// One `every` or `after` a behavior declares.
///
/// The slot holds the time **remaining**, not a deadline. An absolute deadline
/// would need a clock the engine does not keep, and would be wrong across a save:
/// a guard half-way through its half-second would resume either immediately or
/// after the whole gap, depending on how long the game was closed. Counting down
/// what is left resumes exactly where it stopped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimerLayout {
    /// Whether it repeats.
    pub kind: TimerKind,
    /// The interval, in seconds.
    pub seconds: f32,
    /// The function its body compiled to.
    pub member: String,
    /// The state that owns it, by discriminant.
    ///
    /// `None` for a schedule written at behavior level, which runs whatever the
    /// behavior is doing. A state's own runs only while it is in that state and
    /// is re-armed on entry — `every 0.5s { Scan(); }` inside `Patrol` means
    /// "every half-second **while patrolling**", which is the whole reason to
    /// write it there rather than beside it.
    pub state: Option<usize>,
}

/// A behavior's field slots, in the order the compiler assigned them.
///
/// # The layout, and why states share their slots
///
/// ```text
/// [0 .. fields.len())      the behavior's own fields — always readable
/// [fields.len()]           which state it is in
/// [fields.len() + 1 ..)    the current state's data
/// ```
///
/// Every state's data occupies the **same** region, because a state's data only
/// exists while the behavior is in it. That is not a saving, it is the point:
/// a patrol's waypoint index cannot be read while chasing because it is not
/// there to read. In C# those variables sit on the class and are always in
/// scope, which is what makes a state machine written that way so easy to get
/// subtly wrong.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct BehaviorLayout {
    /// The behavior's name.
    pub name: String,
    /// Field names, index = slot.
    pub fields: Vec<String>,
    /// Its states, in declaration order — the index is the discriminant.
    pub states: Vec<StateLayout>,
    /// Its `every` and `after` members, in declaration order.
    pub timers: Vec<TimerLayout>,
}

impl BehaviorLayout {
    /// The slot a field name occupies.
    pub fn slot_of(&self, field: &str) -> Option<usize> {
        self.fields.iter().position(|known| known == field)
    }

    /// The slot holding which state the behavior is in.
    ///
    /// Sits between the behavior's fields and the state's, so adding a field
    /// moves it — which is exactly why a reload matches by name and never by
    /// position.
    pub fn state_slot(&self) -> usize {
        self.fields.len()
    }

    /// Where a state's own data begins.
    ///
    /// A behavior with no states reserves no discriminant: there is nothing to
    /// record, and a slot held for a state that cannot exist would be one more
    /// thing a reload has to carry across for no reason.
    pub fn state_data_slot(&self) -> usize {
        self.fields.len() + usize::from(!self.states.is_empty())
    }

    /// Where the timers' countdowns begin, after the widest state.
    pub fn timer_slot(&self, index: usize) -> usize {
        let widest = self.states.iter().map(|s| s.slots.len()).max().unwrap_or(0);
        self.state_data_slot() + widest + index
    }

    /// How many slots an instance of this behavior needs.
    ///
    /// Sized for the largest state, since only one is ever live, plus one
    /// countdown per timer.
    pub fn slot_count(&self) -> usize {
        self.timer_slot(self.timers.len())
    }

    /// The discriminant a state name compiles to.
    pub fn state_index(&self, name: &str) -> Option<usize> {
        self.states.iter().position(|state| state.name == name)
    }

    /// The state a discriminant names.
    pub fn state_at(&self, index: usize) -> Option<&StateLayout> {
        self.states.get(index)
    }
}

/// A whole compiled module.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Program {
    /// Functions, addressed by index.
    pub functions: Vec<Function>,
    /// What each behavior's field slots are called.
    ///
    /// The running code addresses a field by slot, which is right — a name
    /// lookup per access would be paid on every frame for something fixed at
    /// compile time. But a slot is only meaningful against the program that
    /// assigned it, and hot-reload replaces exactly that: after an edit, slot 1
    /// may be a different field, or the same field moved. Matching the old
    /// layout to the new one by name is what lets an instance keep the values
    /// that still mean something, and the names have to be recorded here for
    /// that comparison to be possible at all.
    pub behaviors: Vec<BehaviorLayout>,
    /// Every string literal the program contains, deduplicated.
    ///
    /// Held once for the whole program rather than per function, and named by
    /// index: a literal then costs a register write wherever it appears, so
    /// `Log("hit")` inside a loop does not allocate once per iteration.
    pub strings: Vec<String>,
}

impl Program {
    /// The index of the function named `name`.
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.functions.iter().position(|f| f.name == name)
    }

    /// The literal at `index`.
    pub fn string(&self, index: u32) -> Option<&str> {
        self.strings.get(index as usize).map(String::as_str)
    }

    /// The field layout of the named behavior.
    pub fn layout(&self, behavior: &str) -> Option<&BehaviorLayout> {
        self.behaviors.iter().find(|b| b.name == behavior)
    }

    /// The function named `name`.
    pub fn function(&self, name: &str) -> Option<&Function> {
        self.functions.iter().find(|f| f.name == name)
    }

    /// A number identifying this program's **code shape**.
    ///
    /// What a suspended machine holds is a position: a function index, a
    /// program counter, a register file sized for that function's frame. None of
    /// those survives an edit that moves code around, and resuming into a
    /// program where they now mean something else would run whatever happens to
    /// sit there. So a saved sequence records this, and is abandoned rather than
    /// resumed when it no longer matches.
    ///
    /// Shape, deliberately, and not the code itself: changing `health = 100` to
    /// `health = 120` leaves every instruction where it was, so a machine
    /// suspended in that function resumes correctly and should not be thrown
    /// away for a number the author retuned. Inserting a statement moves
    /// everything after it, and does change this.
    pub fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for function in &self.functions {
            function.name.hash(&mut hasher);
            function.arity.hash(&mut hasher);
            function.registers.hash(&mut hasher);
            function.code.len().hash(&mut hasher);
        }
        hasher.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty(name: &str) -> Function {
        Function {
            name: name.to_owned(),
            arity: 0,
            registers: 1,
            code: vec![Instruction::Halt],
        }
    }

    #[test]
    fn functions_resolve_by_name_to_an_index() {
        let program = Program {
            strings: Vec::new(),
            behaviors: Vec::new(),
            functions: vec![empty("First"), empty("Second")],
        };
        assert_eq!(program.index_of("Second"), Some(1));
        assert_eq!(program.index_of("Missing"), None);
        assert_eq!(
            program.function("First").map(|f| f.name.as_str()),
            Some("First")
        );
    }
}
