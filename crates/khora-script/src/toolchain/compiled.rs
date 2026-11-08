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

//! What one compile hands back.

use crate::reload::ScriptReload;

use super::ScriptDeclarations;

/// What a [`ScriptCompiler`](super::ScriptCompiler) made of one request:
/// the reloads — a program per module that compiled, a withdrawal per module
/// gone — and what each module recompiled declares now. A module that does
/// not compile is in neither: what it ran before keeps running.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Compiled {
    /// The modules recompiled or withdrawn.
    pub reloads: Vec<ScriptReload>,
    /// What the modules declare now, one set per declaring module.
    pub declarations: Vec<ScriptDeclarations>,
}

impl Compiled {
    /// How many modules compiled to a program: a withdrawal, or a module
    /// that did not compile, is not counted.
    pub fn programs(&self) -> usize {
        self.reloads
            .iter()
            .filter(|reload| reload.program.is_some())
            .count()
    }
}
