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

//! The script toolchain, as the engine drives it off the frame: what a
//! compile is asked ([`CompileRequest`]), who answers it
//! ([`ScriptCompiler`]), and what it hands back ([`Compiled`]) — the
//! recompiled programs and what their modules declare
//! ([`ScriptDeclarations`]), both staged for a frame's boundary.
//!
//! The compile machinery itself — reading a project's files, its importers,
//! the engine's prelude, where problems are reported — lives with the
//! project, behind [`ScriptCompiler`]; this module holds only the words the
//! compile family and the project share.

mod compile_request;
mod compiled;
mod script_compiler;
mod script_declarations;

pub use compile_request::CompileRequest;
pub use compiled::Compiled;
pub use script_compiler::ScriptCompiler;
pub use script_declarations::ScriptDeclarations;

#[cfg(test)]
mod tests;
