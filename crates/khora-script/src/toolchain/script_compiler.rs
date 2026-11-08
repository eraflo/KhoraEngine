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

//! Who compiles a project's scripts for the `loading` family.

use khora_core::lane::Lease;

use super::{CompileRequest, Compiled};

/// Compiles what a [`CompileRequest`] names, as the project lays its
/// scripts out. Called off the frame, from the `loading` family's lanes: it
/// touches no `World`, and whatever it reports to (problems, the script
/// flow's demand) is thread-safe.
pub trait ScriptCompiler: Send + Sync {
    /// Compiles `request`'s modules and their importers.
    fn compile(&self, request: &CompileRequest) -> Compiled;

    /// Compiles `request` as [`compile`](Self::compile) does, and says what
    /// the result holds to: a [`Lease`] that stops holding once what it was
    /// compiled for is gone — its project closed while the compile was out.
    /// The reserve then abandons it rather than install it. None, by
    /// default: a compiler that knows of no project hands on what it
    /// compiled whatever happens.
    fn compile_leased(&self, request: &CompileRequest) -> (Compiled, Option<Lease>) {
        (self.compile(request), None)
    }
}
