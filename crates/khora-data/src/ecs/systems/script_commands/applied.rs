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

//! Which of a frame's commands applied, and where each fell in the frame: what
//! a behavior's writeback orders its turn's writes against.

use std::collections::HashMap;

use khora_core::ecs::entity::EntityId;
use khora_core::script::{WorldCommand, WriteTargetRef};

/// For each component field, the positions among the frame's commands of the
/// writes to it that applied, in order — and for each component written
/// whole, the position of the last such write.
#[derive(Debug, Default)]
pub struct AppliedWrites {
    fields: HashMap<(EntityId, String, String), Vec<usize>>,
    whole: HashMap<(EntityId, String), usize>,
}

impl AppliedWrites {
    /// Records that `command`, the frame's `index`th, applied.
    pub fn record(&mut self, index: usize, command: &WorldCommand) {
        command.for_each_write_target(|entity, target| match target {
            WriteTargetRef::Field(component, field) => {
                self.fields
                    .entry((entity, component.as_str().to_owned(), field.to_owned()))
                    .or_default()
                    .push(index);
            }
            WriteTargetRef::Component(component) => {
                self.whole
                    .insert((entity, component.as_str().to_owned()), index);
            }
            _ => {}
        });
    }

    /// Whether a command from `from` on — written after a turn — wrote
    /// `field` of `component` on `entity` and applied.
    pub fn field_written_from(
        &self,
        entity: EntityId,
        component: &str,
        field: &str,
        from: usize,
    ) -> bool {
        self.fields
            .get(&(entity, component.to_owned(), field.to_owned()))
            .and_then(|written| written.last())
            .is_some_and(|&index| index >= from)
    }

    /// Whether `component` on `entity` was written whole this frame — added,
    /// replaced or removed — and the write applied.
    pub fn whole_written(&self, entity: EntityId, component: &str) -> bool {
        self.whole.contains_key(&(entity, component.to_owned()))
    }

    /// Every field written this frame, with the positions of its applied
    /// writes in order: entity, component, field, positions.
    pub fn fields(&self) -> impl Iterator<Item = (EntityId, &str, &str, &[usize])> {
        self.fields
            .iter()
            .map(|((entity, component, field), written)| {
                (
                    *entity,
                    component.as_str(),
                    field.as_str(),
                    written.as_slice(),
                )
            })
    }

    /// Whether a command from `from` on wrote `component` on `entity` whole —
    /// added, replaced or removed it — and applied.
    pub fn whole_written_from(&self, entity: EntityId, component: &str, from: usize) -> bool {
        self.whole
            .get(&(entity, component.to_owned()))
            .is_some_and(|&index| index >= from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::script::{ComponentName, ScriptValue};

    fn entity(index: u32) -> EntityId {
        EntityId {
            index,
            generation: 0,
        }
    }

    fn set(target: EntityId, field: &str) -> WorldCommand {
        WorldCommand::SetComponent {
            entity: target,
            component: ComponentName::new("Guard"),
            value: ScriptValue::Struct(vec![(field.to_owned(), ScriptValue::Int(1))]),
        }
    }

    #[test]
    fn a_field_write_is_found_from_its_index_on_and_not_after() {
        let mut applied = AppliedWrites::default();
        applied.record(3, &set(entity(1), "health"));
        assert!(applied.field_written_from(entity(1), "Guard", "health", 3));
        assert!(applied.field_written_from(entity(1), "Guard", "health", 0));
        assert!(!applied.field_written_from(entity(1), "Guard", "health", 4));
        assert!(!applied.field_written_from(entity(1), "Guard", "speed", 0));
        assert!(!applied.field_written_from(entity(2), "Guard", "health", 0));
        assert!(!applied.whole_written_from(entity(1), "Guard", 0));
    }
}
