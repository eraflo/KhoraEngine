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

/// A behavior's field slots, in the order the compiler assigned them.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct BehaviorLayout {
    /// The behavior's name.
    pub name: String,
    /// Field names, index = slot.
    pub fields: Vec<String>,
}

impl BehaviorLayout {
    /// The slot a field name occupies.
    pub fn slot_of(&self, field: &str) -> Option<usize> {
        self.fields.iter().position(|known| known == field)
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
