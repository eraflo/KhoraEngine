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
//! [`Channel<ScriptReload>`](khora_core::event::Channel) the agent declares
//! and drains.

use khora_core::event::Supersedes;

use crate::vm::Program;

/// One module that finished recompiling, or that is gone.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptReload {
    /// The module's path, which is its identity in the runtime.
    pub module: String,
    /// What it compiled to — or `None`: the module is gone, and its program
    /// leaves the runtime.
    pub program: Option<Program>,
}

impl Supersedes for ScriptReload {
    const COALESCES: bool = true;

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
    use khora_core::event::{Channel, WhenFull};

    fn reload(module: &str) -> ScriptReload {
        ScriptReload {
            module: module.to_owned(),
            program: Some(Program::default()),
        }
    }

    fn withdrawal(module: &str) -> ScriptReload {
        ScriptReload {
            module: module.to_owned(),
            program: None,
        }
    }

    #[test]
    fn saving_twice_before_a_frame_queues_one_reload() {
        let pending = Channel::bounded(8, WhenFull::DropOldest);
        pending.send(reload("ai/guard.erg"));

        let mut newer = reload("ai/guard.erg");
        if let Some(program) = newer.program.as_mut() {
            program.strings.push("second".to_owned());
        }
        pending.send(newer);

        let drained = pending.drain();
        assert_eq!(drained.len(), 1);
        assert_eq!(
            drained[0]
                .program
                .as_ref()
                .map(|program| program.strings.clone()),
            Some(vec!["second".to_owned()])
        );
    }

    /// A file deleted after it was saved, before a frame drained the queue:
    /// the frame withdraws the module rather than install what was deleted.
    #[test]
    fn a_withdrawal_supersedes_a_queued_reload_of_its_module() {
        let pending = Channel::bounded(8, WhenFull::DropOldest);
        pending.send(reload("ai/guard.erg"));
        pending.send(withdrawal("ai/guard.erg"));

        assert_eq!(pending.drain(), vec![withdrawal("ai/guard.erg")]);
    }

    /// A file deleted then brought back before a frame drained the queue:
    /// the frame installs what came back.
    #[test]
    fn a_reload_supersedes_a_queued_withdrawal_of_its_module() {
        let pending = Channel::bounded(8, WhenFull::DropOldest);
        pending.send(withdrawal("ai/guard.erg"));
        pending.send(reload("ai/guard.erg"));

        assert_eq!(pending.drain(), vec![reload("ai/guard.erg")]);
    }

    /// Withdrawing one module leaves another's queued reload in place.
    #[test]
    fn a_withdrawal_leaves_another_modules_reload_queued() {
        let pending = Channel::bounded(8, WhenFull::DropOldest);
        pending.send(reload("loot/chest.erg"));
        pending.send(withdrawal("ai/guard.erg"));

        assert_eq!(
            pending.drain(),
            vec![reload("loot/chest.erg"), withdrawal("ai/guard.erg")]
        );
    }

    #[test]
    fn different_modules_queue_separately() {
        let pending = Channel::bounded(8, WhenFull::DropOldest);
        pending.send(reload("ai/guard.erg"));
        pending.send(reload("loot/chest.erg"));

        assert_eq!(pending.drain().len(), 2);
    }
}
