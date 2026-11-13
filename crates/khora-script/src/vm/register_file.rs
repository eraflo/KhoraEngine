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

//! Register access: each frame's window into the flat register file, read
//! and written by index, and the typed reads the instructions use.

use super::{Fault, Machine, Reg, Value};

impl Machine {
    pub(super) fn slot(&self, index: Reg) -> Result<usize, Fault> {
        let base = self.frames.last().map(|f| f.base).unwrap_or(0);
        let slot = base + index as usize;
        if slot >= self.registers.len() {
            return Err(Fault::BadRegister { index });
        }
        Ok(slot)
    }

    pub(super) fn read(&self, index: Reg) -> Result<Value, Fault> {
        Ok(self.registers[self.slot(index)?])
    }

    pub(super) fn write(&mut self, index: Reg, value: Value) -> Result<(), Fault> {
        let slot = self.slot(index)?;
        self.registers[slot] = value;
        Ok(())
    }

    pub(super) fn int(&self, index: Reg) -> Result<i64, Fault> {
        let value = self.read(index)?;
        value.as_int().ok_or(Fault::TypeMismatch {
            expected: "int",
            found: value.type_name(),
        })
    }

    pub(super) fn float(&self, index: Reg) -> Result<f32, Fault> {
        let value = self.read(index)?;
        value.as_float().ok_or(Fault::TypeMismatch {
            expected: "float",
            found: value.type_name(),
        })
    }
}
