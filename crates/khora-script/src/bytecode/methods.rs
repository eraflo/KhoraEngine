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

//! `xs.Push(v)` and `xs.RemoveAt(i)`: growing and shrinking an array in place.
//!
//! The receiver is a place — a variable, a field, or an element or struct
//! field of one; the checker refused anything else. Like any write through a
//! path, the index operands and the argument are evaluated first, then the
//! path is walked and the array changed, and a field root is written back
//! with no stop in between.
//!
//! `Push` copies an array or struct it is given (value semantics). `RemoveAt`
//! shifts what follows the removed element, and on a variable's array is
//! charged for it and is a checkpoint; inside an update of a field, the
//! field's load paid for the array and the update does not stop halfway.

use super::objects::At;
use super::{Compiler, Shape};
use crate::ast::Expr;
use crate::types::Ty;
use crate::vm::{Instruction, Reg, Value};

impl Compiler {
    /// `object.method(args)` — or, `optional`, `object?.method(args)` — when
    /// `object` is an array, or `None`.
    pub(super) fn compile_array_method(
        &mut self,
        object: &Expr,
        method: &str,
        args: &[Expr],
        optional: bool,
        span: crate::diagnostics::Span,
    ) -> Option<(Reg, Shape)> {
        let array = match self.type_of(object) {
            Some(Ty::Array(_)) => true,
            Some(Ty::Optional(inner)) => matches!(**inner, Ty::Array(_)),
            _ => false,
        };
        if !array {
            return None;
        }
        // The index operands once, before anything else of the call.
        let Some((root, at)) = self.place_path(object) else {
            self.error(
                format!("`{method}` needs an array kept in a variable"),
                span,
            );
            return Some(self.constant(Value::Unit, Shape::Other));
        };
        let Some(argument) = args.first() else {
            return Some(self.constant(Value::Unit, Shape::Other));
        };

        // `?.`: nothing at all — the argument included — when the array is
        // absent. Tested on a walk of its own; the walk that changes the
        // array comes after the argument and tests again, since another body
        // may have emptied the place while this one waited.
        let mut absent = Vec::new();
        if optional {
            let mark = self.registers.mark();
            let (start, _) = self.load_root(&root);
            let found = self.walk_to(start, &at);
            absent.push(self.emit(Instruction::JumpIfNull {
                src: found,
                target: usize::MAX,
            }));
            self.registers.release_to(mark);
        }

        let held = self.registers.temp();
        let inner = self.registers.mark();
        let (value, _) = match method {
            "Push" => self.compile_value(argument),
            _ => self.compile_expr(argument),
        };
        if value != held {
            self.emit(Instruction::Move {
                dst: held,
                src: value,
            });
        }
        self.registers.release_to(inner);

        // A removal from a variable's array is charged what it shifts, and a
        // run that cannot pay stops here, before the path is walked: walking
        // it again on resuming takes fresh references into the array.
        let charged = method == "RemoveAt" && self.lookup_local(&root).is_some();
        let from = self.here();
        if charged {
            self.record_checkpoint("remove");
        }
        let (start, field) = self.load_root(&root);
        let array = self.walk_to(start, &at);
        if optional {
            absent.push(self.emit(Instruction::JumpIfNull {
                src: array,
                target: usize::MAX,
            }));
        }
        match method {
            "Push" => self.emit(Instruction::Push { array, src: held }),
            _ => self.emit(Instruction::RemoveAt {
                array,
                index: held,
                charged: charged && field.is_none(),
                from,
            }),
        };
        if let Some(slot) = field {
            self.emit(Instruction::WriteBack { slot, src: start });
        }
        for jump in absent {
            self.patch_to_here(jump);
        }
        Some(self.constant(Value::Unit, Shape::Other))
    }

    /// The value at the end of `at`, walked from `start`.
    fn walk_to(&mut self, start: Reg, at: &[At]) -> Reg {
        at.iter()
            .fold(start, |current, &place| self.step_into(current, place))
    }
}
