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

//! # Ergon — the Khora scripting language
//!
//! Ergon exists for one reason no off-the-shelf language satisfies: the engine's
//! [`Agent`] contract requires a subsystem to *degrade on demand*. The DCC hands
//! out a frame budget, and whatever consumes it must be able to stop partway,
//! hand back control, and continue next frame having lost nothing. Lua, Rhai and
//! WASM run to completion or they fail; none of them takes orders from an
//! external scheduler.
//!
//! So the virtual machine is **interruptible by construction**. That single
//! property pays for three separate features:
//!
//! | Reason to suspend | Resumes |
//! |---|---|
//! | Out of fuel (the GORNA budget) | as soon as the budget allows |
//! | `await <duration>` | when it elapses |
//! | `await <event>` | when it resolves |
//!
//! One mechanism, three uses. Declarative time (`every 0.5s`) and `async/await`
//! are not separate systems bolted on — they are the budget mechanism wearing a
//! different hat.
//!
//! ## State of the crate
//!
//! Phase 0: the suspend/resume core, proven before any syntax exists. There is
//! no lexer, parser or type checker yet — deliberately. If a program cannot be
//! stopped and restarted without changing its result, nothing built on top would
//! be worth writing.
//!
//! [`Agent`]: khora_core::agent::Agent

#![warn(missing_docs)]

// `#[ergon_fn]` writes `::khora_script::…` paths, which have to resolve inside
// this crate too — the built-ins are declared with the same macro a game uses,
// so the macro is exercised by the language's own code rather than only by its
// consumers.
extern crate self as khora_script;

pub mod arena;
pub mod ast;
pub mod bytecode;
pub mod diagnostics;
pub mod dispatch;
#[cfg(test)]
mod dispatch_tests;
pub mod lexer;
pub mod lifecycle;
pub mod modules;
pub mod native;
pub mod parser;
pub mod types;
pub mod vm;

// Re-exported for `#[ergon_fn]`, whose expansion submits through it. A crate
// exposing a function to Ergon should need `khora-script` and nothing else;
// without this it would have to declare `inventory` too, for a reason visible
// only in a macro's output.
pub use inventory;

pub use arena::{Arena, ArenaRef, Object, PersistentStore};
pub use ast::{BehaviorDecl, Expr, Item, Module, Stmt, TypeRef};
pub use bytecode::{compile, Compiled};
pub use diagnostics::{Diagnostic, Severity, SourceFile, Span};
pub use dispatch::{deliver, handles, NotDelivered};
pub use khora_macros::ergon_fn;
pub use lexer::{lex, Keyword, Lexed, Token, TokenKind};
pub use modules::{resolve, MemoryLoader, Resolved, SourceLoader};
pub use native::{
    Host, NativeContext, NativeError, NativeFn, NativeRegistry, NativeTy, ScriptType,
};
pub use parser::{parse, Parsed};
pub use types::{check, Checked, Ty};
pub use vm::{Function, Instruction, Machine, Program, Run, Suspension, Value};
