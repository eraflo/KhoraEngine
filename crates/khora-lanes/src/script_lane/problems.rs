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

//! What is wrong with the running instances, for a tool to show: each frame
//! the scripting world changed, the `Running` source of the project's
//! [`ScriptProblems`] is rebuilt from it — an error for every instance a fault
//! disabled, a warning for every body running past the watchdog's first
//! limit, and one for every event an instance was sent with a payload its
//! handler could not take. An instance re-enabled, or gone, leaves the list
//! with it.

use std::sync::{Arc, Mutex};

use khora_core::lane::OutputDeck;
use khora_core::script::{ProblemSeverity, ProblemSource, ScriptProblem, ScriptProblems};
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, TickPhase, World};

use khora_core::ecs::entity::EntityId;

use super::runtime::{member_of, Instance, ScriptRuntime};
use super::watchdog::WATCHDOG_WARN;

impl ScriptRuntime {
    /// What is wrong with the instances held now, in entity order.
    pub fn running_problems(&self) -> Vec<ScriptProblem> {
        let mut held: Vec<_> = self.instances.iter().collect();
        held.sort_by(|((a, a_name), _), ((b, b_name), _)| {
            (a.index, a.generation, a_name).cmp(&(b.index, b.generation, b_name))
        });
        held.into_iter()
            .flat_map(|((entity, behavior), instance)| {
                let module = (!instance.module.is_empty()).then(|| instance.module.clone());
                let dropped = instance.dropped.iter().map(move |(event, why)| ScriptProblem {
                    severity: ProblemSeverity::Warning,
                    module: module.clone(),
                    line: 0,
                    column: 0,
                    message: format!("`{behavior}` was sent `{event}` it could not take: {why}"),
                    note: Some(
                        "a script's raise is checked against the event; this one came from the engine, or was queued before the event changed".to_owned(),
                    ),
                    entity: Some(*entity),
                    behavior: Some(behavior.clone()),
                });
                self.instance_problem(*entity, behavior, instance)
                    .into_iter()
                    .chain(dropped)
            })
            .collect()
    }

    /// An instance's fault or long-running body, if it has one.
    fn instance_problem(
        &self,
        entity: EntityId,
        behavior: &str,
        instance: &Instance,
    ) -> Option<ScriptProblem> {
        let module = (!instance.module.is_empty()).then(|| instance.module.clone());
        let problem = |severity, message: String, note: Option<String>| ScriptProblem {
            severity,
            module: module.clone(),
            line: 0,
            column: 0,
            message,
            note,
            entity: Some(entity),
            behavior: Some(behavior.to_owned()),
        };
        if instance.disabled {
            let fault = instance.fault.as_ref()?;
            return Some(problem(
                ProblemSeverity::Error,
                format!("`{behavior}` faulted and was disabled: {}", fault.reason),
                Some("an edit to its module runs it again".to_owned()),
            ));
        }
        let pending = instance
            .pending
            .as_ref()
            .filter(|pending| pending.running >= WATCHDOG_WARN)?;
        let program = self.program(&instance.module)?;
        Some(problem(
            ProblemSeverity::Warning,
            format!(
                "`{behavior}`'s `{}` has run {:.0} s of game time without finishing — at {}",
                member_of(pending, program),
                pending.running,
                pending.machine.standing(program)
            ),
            Some("a loop with no `await` never ends; the watchdog stops it at 60 s".to_owned()),
        ))
    }
}

fn script_problems(_world: &mut World, runtime: &Runtime, _deck: &mut OutputDeck) {
    let Some(problems) = runtime.resources.get::<Arc<ScriptProblems>>() else {
        return;
    };
    let Some(scripts) = runtime.services.get::<Arc<Mutex<ScriptRuntime>>>() else {
        return;
    };
    let running = match scripts.lock() {
        Ok(scripts) => scripts.running_problems(),
        Err(poisoned) => poisoned.into_inner().running_problems(),
    };
    // Unchanged lists change nothing, so a quiet frame costs the walk only.
    problems.report(ProblemSource::Running, running);
}

khora_script::inventory::submit! {
    DataSystemRegistration {
        name: "script_problems",
        phase: TickPhase::Maintenance,
        // After the writeback: what it reads is the frame's last word.
        order_hint: 40,
        run: script_problems,
        runs_after: &["behavior_writeback"],
    }
}
