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
//! Free functions and behavior members. A member is not special at run time: it
//! is a function named `Guard.Damaged`, compiled with the behavior's field
//! table in scope. Fields do not become registers — a register file belongs to
//! a call, and a field outlives every call made on the entity — so they compile
//! to `LoadField`/`StoreField` against the persistent store.
//!
//! `state`, `every`, `after` and `await` arrive with the rest of the execution
//! model: each needs somewhere for a suspension or a state to live, and
//! compiling `become` before that exists would be inventing a target.

pub mod expr;
pub mod registers;
pub mod stmt;

#[cfg(test)]
mod tests;

pub use registers::Registers;

use std::collections::HashMap;

use crate::ast::{BehaviorMember, Item, Module, TypeRef};
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
    compile_with(module, &crate::native::NativeRegistry::with_builtins())
}

/// Compiles a module against a specific set of engine functions.
///
/// The resulting program addresses a native **by index**, so it is only valid
/// against a registry built the same way. Pass the same one here and to
/// [`check_with`](crate::types::check_with), and hand the same one to the
/// [`Host`](crate::native::Host) that runs the program — a registry assembled
/// differently would resolve a call to whatever now sits at that slot.
pub fn compile_with(module: &Module, natives: &crate::native::NativeRegistry) -> Compiled {
    let mut compiler = Compiler::new();
    compiler.collect_natives(natives);
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
    /// Text. Its own shape because `+` on two strings is not an addition — it
    /// allocates, and the compiler has to know before it picks an instruction.
    Str,
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
    /// Engine function name to its index in the registry, and its result shape.
    pub natives: HashMap<String, (usize, Shape)>,
    /// Fields of the behavior being compiled, by name.
    ///
    /// Empty while compiling a free function, which is what makes a stray
    /// field name there an ordinary "no such variable" rather than a silent
    /// read of slot zero.
    pub fields: HashMap<String, (u16, Shape)>,
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
            natives: HashMap::new(),
            fields: HashMap::new(),
            code: Vec::new(),
            registers: Registers::new(0),
            locals: Vec::new(),
        }
    }

    /// Records a problem the compiler cannot work around.
    pub fn error(&mut self, message: impl Into<String>, span: Span) {
        self.diagnostics.push(Diagnostic::error(message, span));
    }

    /// Records the registry's indices, which the emitted code addresses.
    fn collect_natives(&mut self, natives: &crate::native::NativeRegistry) {
        for (index, native) in natives.iter().enumerate() {
            let shape = match native.result {
                crate::native::NativeTy::Int => Shape::Int,
                crate::native::NativeTy::Float
                | crate::native::NativeTy::Duration
                | crate::native::NativeTy::Angle => Shape::Float,
                _ => Shape::Other,
            };
            self.natives.insert(native.name.to_owned(), (index, shape));
        }
    }

    /// Assigns an index to every function before compiling any body.
    ///
    /// Walks the items in **the order the bodies will be emitted**, behavior
    /// members included. Counting only free functions would be right until a
    /// behavior sat above one in the file, at which point every index after it
    /// would name a different function than the caller meant — the kind of
    /// mistake that produces a working program and a wrong one.
    fn collect_signatures(&mut self, module: &Module) {
        let mut index = 0;
        for item in &module.items {
            match item {
                Item::Function(decl) => {
                    self.signatures.insert(decl.name.clone(), index);
                    self.returns
                        .insert(decl.name.clone(), shape_of(&decl.return_ty));
                    index += 1;
                }
                Item::Behavior(decl) => {
                    // The field initialiser is emitted first, so it takes the
                    // index before any member.
                    self.signatures.insert(init_name(&decl.name), index);
                    self.returns.insert(init_name(&decl.name), Shape::Other);
                    index += 1;

                    for member in &decl.members {
                        let (name, returns) = match member {
                            BehaviorMember::Method(method) => (
                                format!("{}.{}", decl.name, method.name),
                                shape_of(&method.return_ty),
                            ),
                            BehaviorMember::Handler(handler) => {
                                (format!("{}.{}", decl.name, handler.event), Shape::Other)
                            }
                            _ => continue,
                        };
                        self.signatures.insert(name.clone(), index);
                        self.returns.insert(name, returns);
                        index += 1;
                    }
                }
                Item::Struct(_) => {}
            }
        }
    }

    fn compile_functions(&mut self, module: &Module) {
        for item in &module.items {
            match item {
                Item::Function(decl) => {
                    self.compile_body(&decl.name, &decl.params, &decl.body);
                }
                Item::Behavior(decl) => self.compile_behavior(decl),
                Item::Struct(_) => {}
            }
        }
    }

    /// Compiles a behavior's members, each as a function of its own.
    ///
    /// A member is not special at run time: `on Damaged(int amount)` becomes a
    /// function taking an `int`, named `Guard.Damaged` so the dispatcher can
    /// find it. What makes it a *member* is the field table in scope while it
    /// compiles — which is why the fields are assigned slots first, before any
    /// body is read, so a member can name a field declared below it.
    fn compile_behavior(&mut self, decl: &crate::ast::BehaviorDecl) {
        self.fields.clear();
        for member in &decl.members {
            if let BehaviorMember::Field(field) = member {
                let slot = self.fields.len() as u16;
                self.fields
                    .insert(field.name.clone(), (slot, shape_of(&field.ty)));
            }
        }

        self.compile_field_defaults(decl);

        for member in &decl.members {
            match member {
                BehaviorMember::Method(method) => {
                    let name = format!("{}.{}", decl.name, method.name);
                    self.compile_body(&name, &method.params, &method.body);
                }
                BehaviorMember::Handler(handler) => {
                    let name = format!("{}.{}", decl.name, handler.event);
                    self.compile_body(&name, &handler.params, &handler.body);
                }
                // `state`, `every` and `after` need the parts of the execution
                // model that are not built yet — becoming a state, and waking on
                // a schedule. Skipping them keeps the program well-formed rather
                // than half-compiled.
                BehaviorMember::Field(_)
                | BehaviorMember::State(_)
                | BehaviorMember::Every(_)
                | BehaviorMember::After(_) => {}
            }
        }

        self.fields.clear();
    }

    /// Emits the function that gives a fresh instance its field values.
    ///
    /// A default is an *expression* — `float speed = 3.0;`, but also
    /// `Sqrt(2.0)` — so only compiled code can produce it. Without this a
    /// declared default would be decoration: the store would hand out unset
    /// slots and the first arithmetic would fault.
    ///
    /// A field with no written default gets its type's zero rather than staying
    /// unset, because `int health;` reads as a number that happens to start at
    /// nothing, not as a hole.
    fn compile_field_defaults(&mut self, decl: &crate::ast::BehaviorDecl) {
        self.code = Vec::new();
        self.locals = Vec::new();
        self.registers = Registers::new(0);

        for member in &decl.members {
            let BehaviorMember::Field(field) = member else {
                continue;
            };
            let Some((slot, _)) = self.fields.get(&field.name).copied() else {
                continue;
            };

            let mark = self.registers.mark();
            let src = match &field.default {
                Some(expr) => self.compile_expr(expr).0,
                None => {
                    let zero = match shape_of(&field.ty) {
                        Shape::Int => crate::vm::Value::Int(0),
                        Shape::Float => crate::vm::Value::Float(0.0),
                        // A string with no default is the empty one, which the
                        // constant table already deduplicates.
                        Shape::Str => {
                            let (register, _) = self.string_constant("");
                            self.emit(Instruction::StoreField {
                                slot,
                                src: register,
                            });
                            self.registers.release_to(mark);
                            continue;
                        }
                        Shape::Other => crate::vm::Value::Unit,
                    };
                    self.constant(zero, Shape::Other).0
                }
            };
            self.emit(Instruction::StoreField { slot, src });
            self.registers.release_to(mark);
        }

        let unit = self.registers.temp();
        self.emit(Instruction::LoadConst {
            dst: unit,
            value: crate::vm::Value::Unit,
        });
        self.emit(Instruction::Return { src: unit });

        let code = std::mem::take(&mut self.code);
        self.program.functions.push(Function {
            name: init_name(&decl.name),
            arity: 0,
            registers: self.registers.frame_size(),
            code,
        });
    }

    /// Compiles one body into a function of the program.
    fn compile_body(&mut self, name: &str, params: &[crate::ast::Param], body: &crate::ast::Block) {
        self.code = Vec::new();
        self.locals = Vec::new();
        self.registers = Registers::new(params.len());

        // Parameters already own registers `0..n` — that is where the VM's
        // calling convention leaves them — so they are recorded rather than
        // allocated. `Registers::new` reserved the slots.
        for (index, param) in params.iter().enumerate() {
            self.locals.push(Local {
                name: param.name.clone(),
                register: index as Reg,
                shape: shape_of(&param.ty),
            });
        }

        for statement in &body.statements {
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
            name: name.to_owned(),
            arity: params.len(),
            registers: self.registers.frame_size(),
            code,
        });
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

/// The function that fills a fresh instance's fields.
///
/// One place, because the compiler writes the name and the loader reads it, and
/// two spellings of the same convention would fail silently — an instance whose
/// fields simply never got their defaults.
pub fn init_name(behavior: &str) -> String {
    format!("{behavior}.__fields")
}

/// The numeric shape a written type compiles to.
pub fn shape_of(ty: &TypeRef) -> Shape {
    match ty {
        TypeRef::Named { name, .. } => match name.as_str() {
            "int" => Shape::Int,
            // Durations and angles are floats at run time: the checker has
            // already proved the units agree, so the arithmetic is the same.
            "float" | "Duration" | "Angle" => Shape::Float,
            "string" => Shape::Str,
            _ => Shape::Other,
        },
        _ => Shape::Other,
    }
}
