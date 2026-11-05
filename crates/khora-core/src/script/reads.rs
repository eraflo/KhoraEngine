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

//! The demand on the script flow: the components scripts read.
//!
//! Owner `"loaded"` holds what the installed programs read; owner
//! `module:<name>` holds what a recompiled module will read, announced by the
//! hot-reload pump before it sends the program and withdrawn once the program
//! is installed. The script flow projects the union.

use super::ComponentName;
use crate::projection::Demand;

/// The components a script reads with `Get` or `Has`, asked of the script
/// flow through an [`Interest<ScriptReads>`](crate::projection::Interest).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScriptReads;

impl Demand for ScriptReads {
    type Item = ComponentName;
}

impl ScriptReads {
    /// The owner holding what the installed programs read.
    pub const LOADED: &'static str = "loaded";

    /// The owner holding what `module`'s incoming program reads, until it is
    /// installed.
    pub fn incoming(module: &str) -> String {
        format!("module:{module}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projection::Interest;

    fn names(list: &[&str]) -> Vec<ComponentName> {
        list.iter().map(|name| ComponentName::new(*name)).collect()
    }

    /// **What scripts read is asked by component name**: the installed
    /// programs' reads and an incoming module's together, sorted by name,
    /// each once — what the script flow projects.
    #[test]
    fn the_script_reads_are_the_component_names_every_owner_wants() {
        let reads = Interest::<ScriptReads>::new();

        reads.announce("loaded", names(&["RigidBody"]));
        reads.announce("module:ai/guard.erg", names(&["Collider", "RigidBody"]));

        assert_eq!(reads.wanted(), names(&["Collider", "RigidBody"]));
        assert!(reads.covers(&names(&["Collider"])));
    }
}
