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

//! Entering a state from outside a script: `Behavior.__enter(int state)`.
//!
//! A fresh instance starts in the first state its behavior declares, and the
//! initialiser enters it. An instance that comes back — from a save, across a
//! hot reload — may be in another, and that state's own data has to start at
//! *its* defaults before the saved values go back on top: a datum the edit
//! added to the state otherwise keeps whatever the first state left in the
//! slot they share. This function is that entry, one branch per state, the
//! same code a `become` into it runs.

use super::keys::state_key;
use super::{Compiler, Registers};
use crate::vm::{Function, Instruction, Value};

/// The function that enters a behavior's state by discriminant.
pub fn enter_name(behavior: &str) -> String {
    format!("{behavior}.__enter")
}

impl Compiler {
    /// Emits `enter_name(behavior)`: for the discriminant it is given, that
    /// state's declared data and schedules, and the discriminant itself.
    pub(super) fn compile_state_entries(&mut self, decl: &crate::ast::BehaviorDecl) {
        self.code = Vec::new();
        self.locals = Vec::new();
        // The discriminant arrives in register 0, the calling convention's
        // first parameter.
        self.registers = Registers::new(1);
        self.locals.push(super::Local {
            name: "state".to_owned(),
            register: 0,
            shape: super::Shape::Int,
            ty: "int".to_owned(),
            scope: "param".to_owned(),
        });
        self.naming.reset();
        self.record_entry();
        self.naming.open_block();

        if let Some(layout) = self.behavior.clone() {
            for (index, state) in layout.states.iter().enumerate() {
                let mark = self.registers.mark();
                let (wanted, _) = self.constant(Value::Int(index as i64), super::Shape::Int);
                let matches = self.registers.temp();
                self.emit(Instruction::Eq {
                    dst: matches,
                    lhs: 0,
                    rhs: wanted,
                });
                let skip = self.emit(Instruction::JumpIfNot {
                    cond: matches,
                    target: usize::MAX,
                });
                self.registers.release_to(mark);

                self.naming
                    .enter_step(&format!("become.{}", state_key(&state.name)));
                self.emit_state_entry(&state.name);
                let slot = layout.state_slot() as u16;
                self.emit(Instruction::StoreField { slot, src: 0 });
                self.naming.leave();
                self.patch_to_here(skip);
            }
        }
        self.naming.close_block();

        let unit = self.registers.temp();
        self.emit(Instruction::LoadConst {
            dst: unit,
            value: Value::Unit,
        });
        self.emit(Instruction::Return { src: unit });

        let code = std::mem::take(&mut self.code);
        self.program.functions.push(Function {
            name: enter_name(&decl.name),
            arity: 1,
            registers: self.registers.frame_size(),
            code,
            fingerprint: 0,
            sites: std::mem::take(&mut self.naming.sites),
        });
    }
}
