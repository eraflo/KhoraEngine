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

//! The components one module declares, on their way to the frame boundary
//! that registers them in the `World`.

use khora_core::event::Supersedes;
use khora_core::lane::Staged;

use crate::vm::DeclaredComponent;

/// The components one module declares, as it last compiled — none when it
/// declares none, or no longer exists.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptDeclarations {
    /// The module, by its path.
    pub module: String,
    /// What it declares.
    pub components: Vec<DeclaredComponent>,
}

impl Supersedes for ScriptDeclarations {
    const COALESCES: bool = true;

    /// A module's newer set replaces its older one: only what the file says
    /// now is registered.
    fn supersedes(&self, earlier: &Self) -> bool {
        self.module == earlier.module
    }
}

/// Staged by the `loading` family, taken by the frame boundary's
/// `script_components`. A set replaced before a boundary took it holds
/// nothing to give back.
impl Staged for ScriptDeclarations {}
