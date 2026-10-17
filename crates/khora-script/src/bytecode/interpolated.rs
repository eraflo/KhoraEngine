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

//! `$"…"` compiled: its holes evaluated left to right into consecutive
//! registers, then one `Format` that writes the text — one allocation,
//! however many holes. How each hole is shown is decided here, from its type.

use super::{Compiler, Shape};
use crate::ast::InterpPiece;
use crate::types::Ty;
use crate::vm::display::{FormatPiece, FormatPlan, Shown};
use crate::vm::{Instruction, Reg};

impl Compiler {
    pub(super) fn compile_interpolated(&mut self, pieces: &[InterpPiece]) -> (Reg, Shape) {
        let holes: Vec<&crate::ast::Expr> = pieces
            .iter()
            .filter_map(|piece| match piece {
                InterpPiece::Hole(hole) => Some(hole),
                InterpPiece::Text(_) => None,
            })
            .collect();
        let dst = self.registers.temp();
        let mark = self.registers.mark();
        // Every slot reserved first: a hole's own temporaries must not land
        // between two of them.
        let base = self.registers.temp();
        let slots: Vec<Reg> = std::iter::once(base)
            .chain((1..holes.len()).map(|_| self.registers.temp()))
            .collect();
        for (hole, slot) in holes.iter().zip(&slots) {
            let inner = self.registers.mark();
            let (value, _) = self.compile_value(hole);
            if value != *slot {
                self.emit(Instruction::Move {
                    dst: *slot,
                    src: value,
                });
            }
            self.registers.release_to(inner);
        }
        let plan = FormatPlan {
            pieces: pieces
                .iter()
                .map(|piece| match piece {
                    InterpPiece::Text(text) => FormatPiece::Text(text.clone()),
                    InterpPiece::Hole(hole) => FormatPiece::Hole(shown(self.type_of(hole))),
                })
                .collect(),
        };
        let index = self.program.formats.len() as u32;
        self.program.formats.push(plan);
        self.emit(Instruction::Format {
            dst,
            base,
            count: holes.len() as u16,
            plan: index,
        });
        self.registers.release_to(mark);
        (dst, Shape::Str)
    }
}

/// How a hole of type `ty` is shown: a number by what it measures.
fn shown(ty: Option<&Ty>) -> Shown {
    match ty {
        Some(Ty::Float) => Shown::Float,
        Some(Ty::Duration) => Shown::Duration,
        Some(Ty::Angle) => Shown::Angle,
        Some(Ty::Optional(inner)) => shown(Some(inner)),
        _ => Shown::Value,
    }
}
