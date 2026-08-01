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

//! Syntax tree to bytecode.
//!
//! Runs **after** the type checker and assumes what it proved. That assumption
//! is what lets the compiler pick `AddInt` or `AddFloat` up front instead of
//! emitting an instruction that inspects its operands: the decision was already
//! made, and paying for it again at every step would be paying twice.
//!
//! # Break points
//!
//! The compiler inserts the suspension points the budget needs. They go at the
//! **top of every loop** and nowhere else in straight-line code, because that
//! is the only place a program can spend unbounded time without returning. A
//! sequence of statements is bounded by its own length; a loop is not.
//!
//! Placing them at loop tops rather than mid-expression is also what keeps the
//! saved state small: at a loop top there are no live temporaries, so a
//! suspension captures locals and a program counter and nothing else.
//!
//! # Scope
//!
//! Free functions today. Behaviors, states and `await` arrive with the
//! execution model, which needs the ECS bridge to mean anything — compiling
//! `become` before there is somewhere for the state to live would be inventing
//! a target.

pub mod expr;
pub mod registers;
pub mod stmt;

#[cfg(test)]
mod tests;

pub use registers::Registers;

use std::collections::HashMap;

use crate::ast::{Item, Module, TypeRef};
use crate::diagnostics::{Diagnostic, Span};
use crate::vm::{Function, Instruction, Program, Reg};

/// What a compilation produced.
#[derive(Debug, Clone)]
pub struct Compiled {
    /// The program. Usable only when `diagnostics` holds no errors.
    pub program: Program,
    /// Problems found.
    pub diagnostics: Vec<Diagnostic>,
}

impl Compiled {
    /// Whether any diagnostic is an error.
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == crate::diagnostics::Severity::Error)
    }
}

/// Compiles a type-checked module.
///
/// Passing a module that failed type checking is a caller mistake: the compiler
/// trusts what the checker proved and will emit nonsense rather than diagnose.
pub fn compile(module: &Module) -> Compiled {
    let mut compiler = Compiler::new();
    compiler.collect_signatures(module);
    compiler.compile_functions(module);
    Compiled {
        program: compiler.program,
        diagnostics: compiler.diagnostics,
    }
}

/// The numeric shape of a value, which is all the compiler needs to pick an
/// instruction.
///
/// Deliberately coarser than [`Ty`](crate::types::Ty): `Duration` and `Angle`
/// are floats here, because once the checker has proved the units agree, the
/// arithmetic is the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// Integer arithmetic.
    Int,
    /// Floating-point arithmetic, including durations and angles.
    Float,
    /// Not a number: bool, null, a struct, void.
    Other,
}

/// A local variable, and the register holding it.
#[derive(Debug, Clone)]
struct Local {
    name: String,
    register: Reg,
    shape: Shape,
}

/// Compiler state.
pub struct Compiler {
    /// The program being built.
    pub program: Program,
    /// Problems found.
    pub diagnostics: Vec<Diagnostic>,
    /// Function name to index, resolved before any body is compiled so a call
    /// forward is as cheap as a call back.
    pub signatures: HashMap<String, usize>,
    /// Return shape per function, to pick the right comparison at a call site.
    pub returns: HashMap<String, Shape>,
    /// Instructions of the function being compiled.
    pub code: Vec<Instruction>,
    /// Register allocation for the current function.
    pub registers: Registers,
    /// Locals in scope, innermost last.
    locals: Vec<Local>,
}

impl Compiler {
    fn new() -> Self {
        Self {
            program: Program::default(),
            diagnostics: Vec::new(),
            signatures: HashMap::new(),
            returns: HashMap::new(),
            code: Vec::new(),
            registers: Registers::new(0),
            locals: Vec::new(),
        }
    }

    /// Records a problem the compiler cannot work around.
    pub fn error(&mut self, message: impl Into<String>, span: Span) {
        self.diagnostics.push(Diagnostic::error(message, span));
    }

    /// Assigns an index to every function before compiling any body.
    fn collect_signatures(&mut self, module: &Module) {
        for item in &module.items {
            if let Item::Function(decl) = item {
                let index = self.signatures.len();
                self.signatures.insert(decl.name.clone(), index);
                self.returns
                    .insert(decl.name.clone(), shape_of(&decl.return_ty));
            }
        }
    }

    fn compile_functions(&mut self, module: &Module) {
        for item in &module.items {
            let Item::Function(decl) = item else {
                // Behaviors need the execution model; skipping them keeps the
                // program well-formed rather than half-compiled.
                continue;
            };

            self.code = Vec::new();
            self.locals = Vec::new();
            self.registers = Registers::new(decl.params.len());

            // Parameters already own registers `0..n` — that is where the VM's
            // calling convention leaves them — so they are recorded rather than
            // allocated. `Registers::new` reserved the slots.
            for (index, param) in decl.params.iter().enumerate() {
                self.locals.push(Local {
                    name: param.name.clone(),
                    register: index as Reg,
                    shape: shape_of(&param.ty),
                });
            }

            for statement in &decl.body.statements {
                self.compile_stmt(statement);
            }

            // A function that falls off its end returns nothing. The VM handles
            // that, but emitting it makes the intent explicit in a disassembly.
            let unit = self.registers.temp();
            self.emit(Instruction::LoadConst {
                dst: unit,
                value: crate::vm::Value::Unit,
            });
            self.emit(Instruction::Return { src: unit });

            let code = std::mem::take(&mut self.code);
            self.program.functions.push(Function {
                name: decl.name.clone(),
                arity: decl.params.len(),
                registers: self.registers.frame_size(),
                code,
            });
        }
    }

    /// Appends an instruction, returning its index.
    pub fn emit(&mut self, instruction: Instruction) -> usize {
        self.code.push(instruction);
        self.code.len() - 1
    }

    /// The index the next instruction will take — a jump target.
    pub fn here(&self) -> usize {
        self.code.len()
    }

    /// Points a previously emitted jump at the current position.
    ///
    /// Forward jumps are emitted before their destination is known, so the
    /// placeholder is patched once the target exists.
    pub fn patch_to_here(&mut self, index: usize) {
        let target = self.here();
        self.patch(index, target);
    }

    /// Points a previously emitted jump at `target`.
    pub fn patch(&mut self, index: usize, target: usize) {
        match self.code.get_mut(index) {
            Some(Instruction::Jump { target: slot })
            | Some(Instruction::JumpIfNot { target: slot, .. }) => *slot = target,
            // Only jumps are ever patched; anything else is a compiler bug, and
            // silently corrupting an unrelated instruction would be worse than
            // leaving it alone.
            _ => {}
        }
    }

    // ── Scopes and locals ─────────────────────────────

    /// Opens a scope, returning the mark to close it with.
    pub fn open_scope(&mut self) -> (usize, usize) {
        (self.locals.len(), self.registers.scope_mark())
    }

    /// Closes a scope, dropping the locals declared inside it.
    pub fn close_scope(&mut self, mark: (usize, usize)) {
        self.locals.truncate(mark.0);
        self.registers.close_scope(mark.1);
    }

    /// Declares a local and returns its register.
    pub fn declare_local(&mut self, name: &str, shape: Shape) -> Reg {
        let register = self.registers.local();
        self.locals.push(Local {
            name: name.to_owned(),
            register,
            shape,
        });
        register
    }

    /// Finds a local, innermost first so an inner declaration shadows an outer.
    pub fn lookup_local(&self, name: &str) -> Option<(Reg, Shape)> {
        self.locals
            .iter()
            .rev()
            .find(|local| local.name == name)
            .map(|local| (local.register, local.shape))
    }
}

/// The numeric shape a written type compiles to.
pub fn shape_of(ty: &TypeRef) -> Shape {
    match ty {
        TypeRef::Named { name, .. } => match name.as_str() {
            "int" => Shape::Int,
            // Durations and angles are floats at run time: the checker has
            // already proved the units agree, so the arithmetic is the same.
            "float" | "Duration" | "Angle" => Shape::Float,
            _ => Shape::Other,
        },
        _ => Shape::Other,
    }
}
