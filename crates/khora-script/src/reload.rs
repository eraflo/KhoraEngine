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

//! A module that finished recompiling, on its way to the agent that runs it.
//!
//! The compiled programs live in the scripting **agent**, which is what lets it
//! declare [`AgentAccess::Isolated`](khora_core::agent::AgentAccess::Isolated).
//! Hot-reload happens on the other side of that line: a file changed, the pump
//! recompiled it, and the result has to cross over. It crosses through a
//! [`Pending<ScriptReload>`](khora_core::script::Pending) the agent declares
//! and drains.

use khora_core::script::Supersedes;

use crate::vm::Program;

/// One module that finished recompiling.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptReload {
    /// The module's path, which is its identity in the runtime.
    pub module: String,
    /// What it compiled to.
    pub program: Program,
}

impl Supersedes for ScriptReload {
    /// Two saves of the same file between frames are one reload: applying the
    /// older after the newer would leave the game running code the author
    /// already replaced.
    fn supersedes(&self, earlier: &Self) -> bool {
        self.module == earlier.module
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::script::Pending;

    fn reload(module: &str) -> ScriptReload {
        ScriptReload {
            module: module.to_owned(),
            program: Program::default(),
        }
    }

    #[test]
    fn saving_twice_before_a_frame_queues_one_reload() {
        let pending = Pending::new();
        pending.push(reload("ai/guard.erg"));

        let mut newer = reload("ai/guard.erg");
        newer.program.strings.push("second".to_owned());
        pending.push(newer);

        let drained = pending.drain();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].program.strings, vec!["second"]);
    }

    #[test]
    fn different_modules_queue_separately() {
        let pending = Pending::new();
        pending.push(reload("ai/guard.erg"));
        pending.push(reload("loot/chest.erg"));

        assert_eq!(pending.drain().len(), 2);
    }
}
