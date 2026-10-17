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

//! Emitting calls: their values placed in consecutive registers, then the
//! call to a script function or an engine function.
//!
//! A call that names nothing and leaves nothing out places its arguments as
//! written. One that does evaluates them in the order written, then places
//! them in its parameters' order, a default loaded where one was left out.

use super::{Compiler, Shape};
use crate::ast::Expr;
use crate::types::Bound;
use crate::vm::{Instruction, Reg};

impl Compiler {
    /// How the checker bound the call at `at`, when it names an argument or
    /// leaves one out.
    pub(super) fn binding_of(&self, at: usize) -> Option<std::rc::Rc<[Bound]>> {
        self.bindings.get(&at).cloned()
    }

    /// Places a bound call's values and emits it: `receiver` and the written
    /// arguments evaluated **in the order written**, each into a register of
    /// its own, then moved into the parameters' consecutive registers in
    /// their order, a default loaded where one was left out.
    ///
    /// Evaluated first, placed after: the side effects follow the source, and
    /// a frame stopped among them holds the same values whatever order the
    /// callee declares its parameters in.
    pub(super) fn emit_bound_call(
        &mut self,
        target: CallTarget,
        name: &str,
        receiver: Option<&Expr>,
        written: &[Expr],
        binding: &[Bound],
    ) -> (Reg, Shape) {
        let (base, argc) = self.place_bound(receiver, written, binding, &[]);
        self.finish_call(target, name, base, argc)
    }

    /// The bound values in consecutive registers, as
    /// [`emit_bound_call`](Self::emit_bound_call) places them; returns the
    /// first and how many. An `int` placed where `floats` says the parameter
    /// is a float arrives as that float.
    pub(super) fn place_bound(
        &mut self,
        receiver: Option<&Expr>,
        written: &[Expr],
        binding: &[Bound],
        floats: &[bool],
    ) -> (Reg, u8) {
        let held: Vec<(Reg, Shape)> = receiver
            .into_iter()
            .chain(written)
            .map(|value| {
                let register = self.registers.temp();
                let inner = self.registers.mark();
                let (source, shape) = self.compile_value(value);
                if source != register {
                    self.emit(Instruction::Move {
                        dst: register,
                        src: source,
                    });
                }
                self.registers.release_to(inner);
                (register, shape)
            })
            .collect();
        let lead = usize::from(receiver.is_some());
        let count = lead + binding.len();
        let base = self.registers.temp();
        let slots: Vec<Reg> = std::iter::once(base)
            .chain((1..count).map(|_| self.registers.temp()))
            .collect();
        if let (Some(&(register, _)), Some(&slot)) =
            (held.first().filter(|_| lead == 1), slots.first())
        {
            self.emit(Instruction::Move {
                dst: slot,
                src: register,
            });
        }
        for (index, bound) in binding.iter().enumerate() {
            let slot = slots[lead + index];
            let inner = self.registers.mark();
            let (source, shape) = match bound {
                Bound::Written(at) => held[lead + at],
                Bound::Default(default) => {
                    let outer = std::mem::replace(&mut self.in_default, true);
                    let compiled = self.compile_value(default);
                    self.in_default = outer;
                    compiled
                }
            };
            if shape == Shape::Int && floats.get(index) == Some(&true) {
                self.emit(Instruction::IntToFloat {
                    dst: slot,
                    src: source,
                });
            } else if source != slot {
                self.emit(Instruction::Move {
                    dst: slot,
                    src: source,
                });
            }
            self.registers.release_to(inner);
        }
        (base, count as u8)
    }

    /// Places the arguments and emits the call.
    ///
    /// Split out because a sibling call resolves differently but is emitted
    /// identically — duplicating the argument placement is exactly how the two
    /// would drift apart.
    pub(super) fn emit_call(
        &mut self,
        target: CallTarget,
        name: &str,
        args: &[&Expr],
    ) -> (Reg, Shape) {
        // Arguments must land in *consecutive* registers: the callee's frame
        // starts at the first of them, so its parameters need no copying.
        //
        // Every slot is reserved before any argument is compiled. Reserving as
        // we go would not work: compiling an argument takes temporaries of its
        // own, so the next slot would no longer be adjacent.
        let base = self.registers.temp();
        let slots: Vec<Reg> = std::iter::once(base)
            .chain((1..args.len()).map(|_| self.registers.temp()))
            .collect();

        for (argument, slot) in args.iter().zip(slots) {
            let mark = self.registers.mark();
            let (value, _) = self.compile_value(argument);
            if value != slot {
                self.emit(Instruction::Move {
                    dst: slot,
                    src: value,
                });
            }
            // The argument's own temporaries are done with; the slot itself
            // sits below the mark and survives.
            self.registers.release_to(mark);
        }

        self.finish_call(target, name, base, args.len() as u8)
    }

    /// Emits the call to `target` of the `argc` values placed from `base`;
    /// returns where its result lands, and its shape.
    fn finish_call(&mut self, target: CallTarget, name: &str, base: Reg, argc: u8) -> (Reg, Shape) {
        let dst = self.registers.temp();
        let shape = match target {
            CallTarget::Script(function) => {
                self.emit(Instruction::Call {
                    function,
                    base,
                    argc,
                    dst,
                });
                // Named by the callee's compiled name, which a sibling call
                // spells without its behavior.
                let callee = self
                    .signatures
                    .iter()
                    .find(|(_, &index)| index == function)
                    .map_or_else(|| name.to_owned(), |(callee, _)| callee.clone());
                self.record_return(&callee, base, dst);
                // By the compiled name too: `returns` is keyed by it, and a
                // sibling's bare name found nothing — its result's shape was
                // lost, and every operation on it picked the wrong form.
                self.returns.get(&callee).copied().unwrap_or(Shape::Other)
            }
            CallTarget::Native(function) => {
                self.emit(Instruction::NativeCall {
                    function,
                    base,
                    argc,
                    dst,
                });
                self.natives
                    .get(name)
                    .map(|(_, shape)| *shape)
                    .unwrap_or(Shape::Other)
            }
        };
        (dst, shape)
    }
}

/// Which kind of call a name resolved to.
///
/// The two are indistinguishable in the source and must not be at the point of
/// emission: they address different tables, and a script function's index used
/// as a native's would call whatever sits at that slot in the registry.
#[derive(Debug, Clone, Copy)]
pub(super) enum CallTarget {
    /// A function compiled from this program.
    Script(usize),
    /// A function the host exposes.
    Native(usize),
}
