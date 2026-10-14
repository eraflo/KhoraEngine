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

//! A system's `ref` rows, from the row's end until they leave as column
//! writes.
//!
//! At a row's end the fields of a `ref` copy that differ from the row as it
//! was read are written into the working set — every later read sees them —
//! and held in the run's batch for that component and those fields. A batch
//! leaves as one [`WorldCommand::WriteColumn`] before any later command of the
//! run on its component, and when the run stops, however it stops: none
//! outlives the run that wrote it, so none is ever saved with a machine, and
//! the run's writes reach the World in the order they were made.

use std::sync::Arc;

use khora_core::ecs::entity::EntityId;
use khora_core::script::{ColumnBatch, ComponentName, ScriptValue, WorldCommand};

use super::{Fault, Instruction, Machine, Program, Reg, Step};
use crate::native::Host;

/// The batches a run has written and not yet sent, in the order each was
/// started.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct PendingRows {
    batches: Vec<(ComponentName, ColumnBatch)>,
}

impl PendingRows {
    /// Holds `entity`'s written `fields` of `component`: in the component's
    /// latest batch when it writes exactly those fields, else in a new one —
    /// a row never joins a batch older than one of its component written
    /// since, which would send it ahead of that one.
    fn push(&mut self, component: &str, entity: EntityId, fields: Vec<(String, ScriptValue)>) {
        let names: Vec<String> = fields.iter().map(|(name, _)| name.clone()).collect();
        let at = self
            .batches
            .iter()
            .rposition(|(held, _)| held.as_str() == component)
            .filter(|&at| self.batches[at].1.fields() == names);
        let at = match at {
            Some(at) => at,
            None => {
                // The names come from one struct, each once.
                let Ok(batch) = ColumnBatch::new(names) else {
                    return;
                };
                self.batches.push((ComponentName::new(component), batch));
                self.batches.len() - 1
            }
        };
        // The width is the batch's own: the row cannot be refused.
        let _ = self.batches[at]
            .1
            .push_row(entity, fields.into_iter().map(|(_, value)| value));
    }

    /// Takes the batches of `component` — every batch when `None` — as the
    /// commands that send them, in the order they were started.
    fn take(&mut self, component: Option<&str>) -> Vec<WorldCommand> {
        let (taken, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.batches)
            .into_iter()
            .partition(|(held, _)| component.is_none_or(|name| held.as_str() == name));
        self.batches = kept;
        taken
            .into_iter()
            .map(|(component, batch)| WorldCommand::WriteColumn {
                component,
                batch: Arc::new(batch),
            })
            .collect()
    }
}

impl Machine {
    /// Executes a [`Instruction::WriteRow`].
    pub(super) fn step_write_row(
        &mut self,
        instruction: &Instruction,
        program: &Program,
        host: &mut Host,
    ) -> Result<Step, Fault> {
        let Instruction::WriteRow {
            entity,
            copy,
            original,
            component,
            ..
        } = *instruction
        else {
            return Ok(Step::Next);
        };
        let entity = self.entity_at(entity)?;
        let name = program.string(component).ok_or(Fault::BadString)?;
        let now = self.row_fields(program, host, copy, name)?;
        let was = self.row_fields(program, host, original, name)?;
        let changed: Vec<(String, ScriptValue)> = now
            .into_iter()
            .filter(|(field, value)| {
                was.iter()
                    .find(|(read, _)| read == field)
                    .is_none_or(|(_, read)| read != value)
            })
            .collect();
        if changed.is_empty() {
            return Ok(Step::Next);
        }
        // Every later read — of this run, and of the scripts after it — sees
        // the row as written.
        host.components.observe(&WorldCommand::SetComponent {
            entity,
            component: ComponentName::new(name),
            value: ScriptValue::Struct(changed.clone()),
        });
        self.rows.push(name, entity, changed);
        Ok(Step::Next)
    }

    /// Sends the run's batches of `component` — all of them when `None` —
    /// ahead of whatever the run queues next.
    pub(super) fn send_rows(&mut self, host: &mut Host, component: Option<&str>) {
        for command in self.rows.take(component) {
            host.commands.push(command);
        }
    }

    /// The fields of the component copy in `register`.
    fn row_fields(
        &self,
        program: &Program,
        host: &Host,
        register: Reg,
        component: &str,
    ) -> Result<Vec<(String, ScriptValue)>, Fault> {
        let unreadable = |reason: String| Fault::Unreadable {
            component: component.to_owned(),
            reason,
        };
        let owned = host
            .arena
            .export(self.read(register)?, program)
            .map_err(|error| unreadable(format!("{error:?}")))?;
        match crate::bridge::from_owned(&owned).map_err(|error| unreadable(error.to_string()))? {
            ScriptValue::Struct(fields) => Ok(fields),
            other => Err(unreadable(format!("a copy holds {other:?}"))),
        }
    }
}
