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

//! Register allocation.
//!
//! A stack discipline, not a graph colouring. Locals sit at the bottom of the
//! frame for as long as their scope lasts; temporaries are taken from the top
//! and released as soon as the expression that needed them is done.
//!
//! That is enough because Ergon expressions are shallow — gameplay code does
//! not nest fifty deep — and because the frame has to be *small*. Every live
//! register is state that gets copied on every suspension, and suspensions are
//! routine here rather than exceptional.

use crate::vm::Reg;

/// Tracks which registers are in use while compiling one function.
#[derive(Debug, Default)]
pub struct Registers {
    /// Registers reserved for named locals, never reclaimed by temporaries.
    locals: usize,
    /// The next free register above the locals.
    next: usize,
    /// The largest `next` ever reached — the frame size the function needs.
    high_water: usize,
}

impl Registers {
    /// An allocator with `parameters` registers already spoken for.
    ///
    /// Parameters occupy `0..parameters` because that is where the VM's calling
    /// convention puts them, so the callee needs no prologue to move them.
    pub fn new(parameters: usize) -> Self {
        Self {
            locals: parameters,
            next: parameters,
            high_water: parameters,
        }
    }

    /// Reserves a register for a named local.
    ///
    /// Locals are stacked below temporaries so that freeing a temporary can
    /// never reclaim a variable that is still in scope.
    pub fn local(&mut self) -> Reg {
        let index = self.locals;
        self.locals += 1;
        self.next = self.next.max(self.locals);
        self.high_water = self.high_water.max(self.next);
        index as Reg
    }

    /// Takes a scratch register.
    pub fn temp(&mut self) -> Reg {
        let index = self.next;
        self.next += 1;
        self.high_water = self.high_water.max(self.next);
        index as Reg
    }

    /// The current top, to be restored by [`Self::release_to`].
    pub fn mark(&self) -> usize {
        self.next
    }

    /// Releases every temporary taken since `mark`.
    ///
    /// Never drops below the locals: a mark taken before a declaration would
    /// otherwise free the variable it declared.
    pub fn release_to(&mut self, mark: usize) {
        self.next = mark.max(self.locals);
    }

    /// Remembers how many locals are live, so a scope can restore it.
    pub fn scope_mark(&self) -> usize {
        self.locals
    }

    /// Drops the locals declared since `mark`, at the end of their scope.
    pub fn close_scope(&mut self, mark: usize) {
        self.locals = mark;
        self.next = self.next.min(self.locals.max(self.next));
        self.next = self.locals;
    }

    /// How many registers the function's frame needs.
    pub fn frame_size(&self) -> usize {
        self.high_water.max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parameters occupy the first registers, matching the calling convention,
    /// so a callee needs no prologue.
    #[test]
    fn parameters_take_the_lowest_registers() {
        let mut registers = Registers::new(2);
        assert_eq!(registers.local(), 2, "the first local sits above them");
        assert_eq!(registers.temp(), 3);
    }

    /// Releasing temporaries must never reclaim a local that is still in scope.
    #[test]
    fn releasing_temporaries_spares_the_locals() {
        let mut registers = Registers::new(0);
        let local = registers.local();
        let mark = registers.mark();
        let temp = registers.temp();
        assert_ne!(local, temp);

        registers.release_to(mark);
        assert_eq!(
            registers.temp(),
            temp,
            "the scratch register is reused, the local is not"
        );
    }

    /// A closed scope gives its locals back, so a loop body does not grow the
    /// frame once per iteration compiled.
    #[test]
    fn closing_a_scope_reclaims_its_locals() {
        let mut registers = Registers::new(0);
        let outer = registers.local();
        let mark = registers.scope_mark();

        let inner = registers.local();
        assert_ne!(outer, inner);

        registers.close_scope(mark);
        assert_eq!(registers.local(), inner, "the slot is reused");
    }

    /// The frame is sized by the peak, not the final state, or a released
    /// temporary would shrink the frame the function still needed.
    #[test]
    fn the_frame_is_sized_by_the_peak() {
        let mut registers = Registers::new(1);
        let mark = registers.mark();
        registers.temp();
        registers.temp();
        registers.temp();
        registers.release_to(mark);
        assert_eq!(registers.frame_size(), 4, "1 parameter + 3 temporaries");
    }

    #[test]
    fn a_frame_is_never_empty() {
        assert_eq!(Registers::new(0).frame_size(), 1);
    }
}
