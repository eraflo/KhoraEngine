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
//! The compiler marks where the budget may stop a run: the start of every
//! statement and the head of every loop, each a `Safepoint` with a name drawn
//! from the source's structure ([`sites`]). The machine also stops at a
//! function's entry and just past a call's return. Between two of these a run
//! spends past its budget if it must; a loop's back edge lands on its head, so
//! that stretch is bounded by the code between two of them and its worst case
//! is recorded as the program's overdraft bound.
//!
//! Stopping only there keeps the saved state small and meaningful: at a
//! statement's start there are no live temporaries, so a suspension captures
//! locals and a program counter — and because the place is named, a machine
//! saved before an edit can be laid out again at the same place after it.
//!
//! # Scope
//!
//! Free functions and behavior members. A member is not special at run time: it
//! is a function named `Guard.Damaged`, compiled with the behavior's field
//! table in scope. Fields do not become registers — a register file belongs to
//! a call, and a field outlives every call made on the entity — so they compile
//! to `LoadField`/`StoreField` against the persistent store.
//!
//! `await` arrives with the suspension half of the execution model: a suspended
//! machine has to be kept per instance, which the schedule below does not need.
//! `state`, `become`, `every` and `after` are here.

mod behavior;
mod components;
mod entry;
pub mod expr;
mod fingerprint;
mod flow;
pub mod keys;
mod methods;
mod objects;
mod operators;
pub mod registers;
pub mod sites;
pub mod stmt;
mod structs;

#[cfg(test)]
mod tests;

pub use entry::enter_name;
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
        crate::diagnostics::has_errors(&self.diagnostics)
    }
}

/// Compiles a type-checked module.
///
/// Passing a module that failed type checking is a caller mistake: the compiler
/// trusts what the checker proved and will emit nonsense rather than diagnose.
pub fn compile(module: &Module) -> Compiled {
    compile_with(module, &crate::native::NativeRegistry::discovered())
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
    // The types `var` declarations inferred: a site records each local's
    // type, and a resumed frame trusts it to tell `int?` from `int`.
    let checked = crate::types::check_with(module, natives);
    compiler.inferred = checked.inferred;
    compiler.types = checked.types;
    compiler.collect_natives(natives);
    compiler.collect_structs(module);
    compiler.collect_signatures(module);
    compiler.compile_functions(module);
    fingerprint::seal(&mut compiler.program, natives);
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
    /// An engine type, by the name a script writes.
    ///
    /// Carried rather than folded into [`Other`](Self::Other) because `v.x` has
    /// to find the accessor that belongs to *this* type — the compiler works in
    /// shapes, so the shape is where the answer has to be.
    Engine(&'static str),
    /// An array: a handle whose binding from a place copies (value
    /// semantics).
    Object,
    /// Not a number: bool, null, a struct, void.
    Other,
}

/// What entering a state has to write into the instance.
///
/// Entering is a real event, not just a change of discriminant: the state's own
/// data starts at whatever its author declared, and its schedules start over.
/// `every 0.5s` inside `Patrol` that resumed a half-spent countdown from the
/// last time the guard patrolled would fire at a moment nothing decided.
#[derive(Debug, Clone, Default)]
pub struct StateEntry {
    /// Its parameters, in order: what a `become` hands it.
    pub params: Vec<(String, TypeRef)>,
    /// Every one of its slots — parameters, then fields — by name, where a
    /// default reads the state's own data from.
    pub slots: Vec<(String, u16, Shape)>,
    /// Its declared fields — the slots after the parameters — and their
    /// defaults. `None` where the author wrote no default.
    pub fields: Vec<(u16, String, Option<crate::ast::Expr>, TypeRef)>,
    /// Its countdown slots, and what each is armed to.
    pub timers: Vec<(u16, f32)>,
}

/// A local variable, and the register holding it.
#[derive(Debug, Clone)]
struct Local {
    name: String,
    register: Reg,
    shape: Shape,
    /// Its type as written, or its shape for one declared with `var` — what a
    /// site records so a frame rebuilt in edited code refuses a local whose
    /// type changed.
    ty: String,
    /// The block that declares it, so a local and one it shadows stay apart.
    scope: String,
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
    /// The compiled names that are a behavior's or a state's *methods* — what a
    /// bare call inside a behavior may name. A handler compiles to the same
    /// form of name (`Guard.Hit` for `on Hit`) and is never called by name.
    pub methods: std::collections::HashSet<String>,
    /// Engine function name to its index in the registry, and its result shape.
    pub natives: HashMap<String, (usize, Shape)>,
    /// The layout of the behavior being compiled, so `become` can resolve a
    /// state name to the discriminant it writes.
    pub behavior: Option<crate::vm::BehaviorLayout>,
    /// The state whose code is being compiled — its members, or the defaults
    /// written on entering it — so a bare call finds that state's methods
    /// first.
    pub state: Option<String>,
    /// Fields of the behavior being compiled, by name.
    ///
    /// Empty while compiling a free function, which is what makes a stray
    /// field name there an ordinary "no such variable" rather than a silent
    /// read of slot zero.
    pub fields: HashMap<String, (u16, Shape)>,
    /// The behavior's own fields, without any state's: the scope a state's
    /// defaults add that state's data to, whichever member enters it.
    pub behavior_fields: HashMap<String, (u16, Shape)>,
    /// The loops around the code being compiled, innermost last — what a
    /// `break` or `continue` acts on.
    loops: Vec<flow::LoopContext>,
    /// How many of [`locals`](Self::locals) a name may not resolve to: the
    /// locals of the member a `become` is written in, while the entered
    /// state's defaults compile — they are still live, and still recorded at
    /// that member's sites, but they are not the state's to read.
    hidden_locals: usize,
    /// What entering each state of the behavior being compiled has to write.
    ///
    /// Collected once per behavior and emitted at every `become`, and again in
    /// the initialiser for the state a fresh instance starts in. Without it a
    /// state's declared defaults were decoration — `Become` wrote the arguments
    /// it was handed and nothing else, so `int missed = 7;` left an unset slot.
    pub entries: HashMap<String, StateEntry>,
    /// Instructions of the function being compiled.
    pub code: Vec<Instruction>,
    /// Register allocation for the current function.
    pub registers: Registers,
    /// Locals in scope, innermost last.
    locals: Vec<Local>,
    /// The sites of the function being compiled, and where the compiler is.
    pub naming: sites::Naming,
    /// What each `var` declaration inferred, from the checker.
    pub inferred: HashMap<usize, String>,
    /// Every expression's type, from the checker — see
    /// [`Checked::types`](crate::types::Checked::types).
    pub types: HashMap<usize, crate::types::Ty>,
    /// The structs the module declares, by name — what a literal fills its
    /// fields from.
    pub struct_decls: HashMap<String, crate::ast::StructDecl>,
    /// The components the program writes, by name — what a write's field is
    /// declared as.
    pub component_decls: HashMap<String, crate::ast::StructDecl>,
}

impl Compiler {
    fn new() -> Self {
        Self {
            program: Program::default(),
            diagnostics: Vec::new(),
            signatures: HashMap::new(),
            returns: HashMap::new(),
            methods: std::collections::HashSet::new(),
            natives: HashMap::new(),
            behavior: None,
            state: None,
            fields: HashMap::new(),
            behavior_fields: HashMap::new(),
            hidden_locals: 0,
            loops: Vec::new(),
            entries: HashMap::new(),
            code: Vec::new(),
            registers: Registers::new(0),
            locals: Vec::new(),
            naming: sites::Naming::default(),
            inferred: HashMap::new(),
            types: HashMap::new(),
            struct_decls: HashMap::new(),
            component_decls: HashMap::new(),
        }
    }

    /// Records a problem the compiler cannot work around.
    pub fn error(&mut self, message: impl Into<String>, span: Span) {
        self.diagnostics.push(Diagnostic::error(message, span));
    }

    /// Records a function's index and return shape, refusing a name twice.
    ///
    /// Two declarations compiling to one name is not a shadowing rule waiting to
    /// be chosen — it is unrepresentable. A direct call resolves through
    /// [`signatures`](Self::signatures), which would hold the *last*, while a
    /// dispatch resolves through [`Program::index_of`], which finds the
    /// **first**: the same name would mean two different bodies depending on how
    /// it was reached. `void Update()` beside `on Update()` is the way an author
    /// meets this without meaning to, since a member's kind is not part of its
    /// compiled name.
    ///
    /// The first declaration is kept, matching what a dispatch would have found,
    /// so the diagnostics that follow describe one coherent program.
    fn declare(&mut self, name: String, returns: Shape, index: usize, span: Span) {
        if self.signatures.contains_key(&name) {
            self.error(
                format!("`{name}` is declared more than once — rename one of the two"),
                span,
            );
            return;
        }
        self.signatures.insert(name.clone(), index);
        self.returns.insert(name, returns);
    }

    /// Records the registry's indices, which the emitted code addresses.
    fn collect_natives(&mut self, natives: &crate::native::NativeRegistry) {
        for (index, native) in natives.iter().enumerate() {
            let shape = match native.result {
                crate::native::NativeTy::Int => Shape::Int,
                crate::native::NativeTy::Float
                | crate::native::NativeTy::Duration
                | crate::native::NativeTy::Angle => Shape::Float,
                crate::native::NativeTy::Str => Shape::Str,
                crate::native::NativeTy::Engine(name) => Shape::Engine(name),
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
                    self.declare(
                        decl.name.clone(),
                        shape_of(&decl.return_ty),
                        index,
                        decl.name_span,
                    );
                    index += 1;
                }
                Item::Behavior(decl) => {
                    // The field initialiser is emitted first, so it takes the
                    // index before any member.
                    self.declare(init_name(&decl.name), Shape::Other, index, decl.name_span);
                    index += 1;
                    // Then the state entry, emitted right after it.
                    self.declare(enter_name(&decl.name), Shape::Other, index, decl.name_span);
                    index += 1;

                    for member in &decl.members {
                        // A state's members are functions too, emitted after
                        // the behavior's own. Counting only the behavior's was
                        // right until a state appeared, at which point every
                        // index after it named a different function.
                        if let BehaviorMember::State(state) = member {
                            for inner in &state.members {
                                let Some((name, returns, span)) =
                                    member_signature(&decl.name, Some(&state.name), inner)
                                else {
                                    continue;
                                };
                                if matches!(inner, BehaviorMember::Method(_)) {
                                    self.methods.insert(name.clone());
                                }
                                self.declare(name, returns, index, span);
                                index += 1;
                            }
                            // Then its scheduled bodies, which `compile_state`
                            // emits in the same place.
                            let owner = format!("{}.{}", decl.name, state.name);
                            for (position, name) in sites::timer_names(&owner, &state.members) {
                                self.declare(
                                    name,
                                    Shape::Other,
                                    index,
                                    state.members[position].span(),
                                );
                                index += 1;
                            }
                            continue;
                        }

                        let Some((name, returns, span)) =
                            member_signature(&decl.name, None, member)
                        else {
                            continue;
                        };
                        if matches!(member, BehaviorMember::Method(_)) {
                            self.methods.insert(name.clone());
                        }
                        self.declare(name, returns, index, span);
                        index += 1;
                    }

                    // Scheduled bodies are emitted last, so they are counted
                    // last — the order here and in `compile_behavior` is the
                    // one contract this pair has.
                    // Named by what each schedule is, so these cannot collide
                    // with each other; `declare` still guards them against an
                    // author's own `__every(1)`.
                    for (position, name) in sites::timer_names(&decl.name, &decl.members) {
                        self.declare(name, Shape::Other, index, decl.members[position].span());
                        index += 1;
                    }
                }
                Item::Struct(_) | Item::Component(_) => {}
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
                Item::Struct(_) | Item::Component(_) => {}
            }
        }
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
                ty: keys::type_name(&param.ty),
                scope: "param".to_owned(),
            });
        }

        self.naming.reset();
        self.record_entry();
        self.compile_statements(&body.statements);
        if self.registers.overflowed() {
            self.error(
                format!(
                    "`{name}` needs more than {} registers — split it into smaller functions",
                    Reg::MAX as usize + 1
                ),
                body.span,
            );
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
        let mut sites = std::mem::take(&mut self.naming.sites);
        // Recorded in emission order already; sorted for a reader that looks a
        // counter up, stably so the first site at a counter stays first.
        sites.sort_by_key(|site| site.pc);
        self.program.functions.push(Function {
            name: name.to_owned(),
            arity: params.len(),
            registers: self.registers.frame_size(),
            code,
            fingerprint: 0,
            sites,
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
            | Some(Instruction::JumpIfNot { target: slot, .. })
            | Some(Instruction::JumpIfNull { target: slot, .. }) => *slot = target,
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

    /// Declares a local of type `ty` (as written) and returns its register.
    pub fn declare_local(&mut self, name: &str, shape: Shape, ty: String) -> Reg {
        let register = self.registers.local();
        let scope = self.naming.scope();
        self.locals.push(Local {
            name: name.to_owned(),
            register,
            shape,
            ty,
            scope,
        });
        register
    }

    /// Hides every local now in scope from lookup, returning what to restore
    /// with [`reveal_locals`](Self::reveal_locals).
    pub fn hide_locals(&mut self) -> usize {
        std::mem::replace(&mut self.hidden_locals, self.locals.len())
    }

    /// Undoes [`hide_locals`](Self::hide_locals).
    pub fn reveal_locals(&mut self, hidden: usize) {
        self.hidden_locals = hidden;
    }

    /// Names a register already holding a value as a local, until the scope
    /// that holds it closes.
    pub fn bind_local(&mut self, name: &str, register: Reg, shape: Shape, ty: String) {
        let scope = self.naming.scope();
        self.locals.push(Local {
            name: name.to_owned(),
            register,
            shape,
            ty,
            scope,
        });
    }

    /// Finds a local, innermost first so an inner declaration shadows an outer.
    pub fn lookup_local(&self, name: &str) -> Option<(Reg, Shape)> {
        self.locals[self.hidden_locals.min(self.locals.len())..]
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
        // An optional holds its type's values or `null`, and only a value
        // ever reaches an instruction that cares — the checker refuses
        // arithmetic on an optional until it is unwrapped.
        TypeRef::Optional { inner, .. } => shape_of(inner),
        TypeRef::Array { .. } => Shape::Object,
        TypeRef::Named { name, .. } => match name.as_str() {
            "int" => Shape::Int,
            // Durations and angles are floats at run time: the checker has
            // already proved the units agree, so the arithmetic is the same.
            "float" | "Duration" | "Angle" => Shape::Float,
            "string" => Shape::Str,
            "bool" | "Entity" => Shape::Other,
            // Borrowed from the checker's list so the name is `'static`, which
            // is what lets a shape carry it. Any other name the checker let
            // through is a struct.
            other => crate::types::ty::ENGINE_TYPES
                .iter()
                .find(|known| **known == other)
                .map_or(Shape::Object, |known| Shape::Engine(known)),
        },
        _ => Shape::Other,
    }
}

/// The numeric shape of a state slot — a parameter's or a field's declared type.
///
/// Looked up by name rather than tracked alongside, because the slot list is
/// what the layout records and the layout is what a reload compares. Two
/// parallel lists would be two things to keep in step.
fn shape_of_state_slot(decl: &crate::ast::StateDecl, slot: &str) -> Shape {
    if let Some(param) = decl.params.iter().find(|param| param.name == slot) {
        return shape_of(&param.ty);
    }
    for member in &decl.members {
        if let BehaviorMember::Field(field) = member {
            if field.name == slot {
                return shape_of(&field.ty);
            }
        }
    }
    Shape::Other
}

/// The compiled name, return shape and source span of a behavior member.
///
/// The name itself comes from [`dispatch`](crate::dispatch), which is where the
/// *reader* of it lives. Both sides claimed to be the one place; neither was,
/// and a handler registered under one spelling and looked up under another does
/// not fail to compile — it simply never fires.
///
/// The span is the *name* rather than the whole declaration: it is what a
/// duplicate report has to point at for the author to see which two collided.
fn member_signature(
    behavior: &str,
    state: Option<&str>,
    member: &BehaviorMember,
) -> Option<(String, Shape, Span)> {
    let qualify = |name: &str| match state {
        Some(state) => crate::dispatch::state_handler_name(behavior, state, name),
        None => crate::dispatch::handler_name(behavior, name),
    };

    match member {
        BehaviorMember::Method(method) => Some((
            qualify(&method.name),
            shape_of(&method.return_ty),
            method.name_span,
        )),
        BehaviorMember::Handler(handler) => {
            Some((qualify(&handler.event), Shape::Other, handler.event_span))
        }
        _ => None,
    }
}

/// The parameters and body of a member that compiles to a function.
///
/// Answers `Some` for exactly the members [`member_signature`] names, so the two
/// cannot disagree about which members become functions — the disagreement would
/// show up as an index counted for a body never emitted, and every function
/// after it resolving to its neighbour.
fn member_body(member: &BehaviorMember) -> Option<(&[crate::ast::Param], &crate::ast::Block)> {
    match member {
        BehaviorMember::Method(method) => Some((&method.params, &method.body)),
        BehaviorMember::Handler(handler) => Some((&handler.params, &handler.body)),
        _ => None,
    }
}
