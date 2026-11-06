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
use khora_core::lane::Staged;

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

/// Staged by the compile family, taken by the scripting family's intake. A
/// reload replaced before the intake took it holds nothing to give back.
impl Staged for ScriptReload {}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::lane::{Reserve, Staging};

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

    /// Stages `reload` into `reserve` as one run's output, handed over at a
    /// frame start.
    fn stage(reserve: &Reserve, reload: ScriptReload) {
        let mut staging = Staging::new();
        staging.stage(vec![reload]);
        reserve.receive(staging);
    }

    #[test]
    fn saving_twice_before_a_boundary_keeps_one_reload() {
        let pending = Reserve::new();
        stage(&pending, reload("ai/guard.erg"));

        let mut newer = reload("ai/guard.erg");
        if let Some(program) = newer.program.as_mut() {
            program.strings.push("second".to_owned());
        }
        stage(&pending, newer);

        let taken = pending.take::<ScriptReload>();
        assert_eq!(taken.len(), 1);
        assert_eq!(
            taken[0]
                .program
                .as_ref()
                .map(|program| program.strings.clone()),
            Some(vec!["second".to_owned()])
        );
    }

    /// A file deleted after it was saved, before a boundary took the reload:
    /// the frame withdraws the module rather than install what was deleted.
    #[test]
    fn a_withdrawal_supersedes_a_waiting_reload_of_its_module() {
        let pending = Reserve::new();
        stage(&pending, reload("ai/guard.erg"));
        stage(&pending, withdrawal("ai/guard.erg"));

        assert_eq!(
            pending.take::<ScriptReload>(),
            vec![withdrawal("ai/guard.erg")]
        );
    }

    /// A file deleted then brought back before a boundary took the
    /// withdrawal: the frame installs what came back.
    #[test]
    fn a_reload_supersedes_a_waiting_withdrawal_of_its_module() {
        let pending = Reserve::new();
        stage(&pending, withdrawal("ai/guard.erg"));
        stage(&pending, reload("ai/guard.erg"));

        assert_eq!(pending.take::<ScriptReload>(), vec![reload("ai/guard.erg")]);
    }

    /// Withdrawing one module leaves another's waiting reload in place.
    #[test]
    fn a_withdrawal_leaves_another_modules_reload_waiting() {
        let pending = Reserve::new();
        stage(&pending, reload("loot/chest.erg"));
        stage(&pending, withdrawal("ai/guard.erg"));

        assert_eq!(
            pending.take::<ScriptReload>(),
            vec![reload("loot/chest.erg"), withdrawal("ai/guard.erg")]
        );
    }

    #[test]
    fn different_modules_wait_separately() {
        let pending = Reserve::new();
        stage(&pending, reload("ai/guard.erg"));
        stage(&pending, reload("loot/chest.erg"));

        assert_eq!(pending.take::<ScriptReload>().len(), 2);
    }
}
