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

//! The components a script may read this frame, and the copy of them its own
//! writes update.
//!
//! The script flow builds a [`ComponentProjection`] of only the components some
//! loaded program reads; the script lane wraps it in [`FrameComponents`], the
//! frame's working set, which every command a script queues updates so a
//! script reads its own writes within the frame.

use std::collections::HashMap;
use std::sync::Arc;

use crate::ecs::entity::EntityId;

use super::{ComponentName, ScriptValue, WorldCommand};

#[cfg(test)]
mod tests;

/// A copy of one component column, read row by row as script values.
pub trait ColumnSnapshot: Send + Sync {
    /// The number of rows.
    fn len(&self) -> usize;

    /// Whether the copy holds no row.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Row `row`, as a `ScriptValue::Struct` of the fields a script reaches.
    fn read(&self, row: usize) -> ScriptValue;
}

/// One read component, as the flow projected it: a snapshot of every page
/// column holding it, and where each live holder's row is.
pub struct ProjectedComponent {
    /// The component.
    pub name: ComponentName,
    /// One snapshot per page holding a column of it.
    columns: Vec<Box<dyn ColumnSnapshot>>,
    /// Live rows only: holder → (column, row).
    index: HashMap<EntityId, (u32, u32)>,
    /// The component's default row, for an `Add` in the working set.
    default: Option<ScriptValue>,
}

impl ProjectedComponent {
    /// A projected component from its snapshots, the index of its live
    /// holders into them, and its default row.
    pub fn new(
        name: ComponentName,
        columns: Vec<Box<dyn ColumnSnapshot>>,
        index: HashMap<EntityId, (u32, u32)>,
        default: Option<ScriptValue>,
    ) -> Self {
        Self {
            name,
            columns,
            index,
            default,
        }
    }

    /// `entity`'s row, converted — `None` when it does not hold the component.
    ///
    /// Only this row is converted: a projection of ten thousand holders read
    /// once costs one conversion.
    pub fn get(&self, entity: EntityId) -> Option<ScriptValue> {
        let &(column, row) = self.index.get(&entity)?;
        Some(self.columns.get(column as usize)?.read(row as usize))
    }

    /// Whether `entity` holds the component.
    pub fn has(&self, entity: EntityId) -> bool {
        self.index.contains_key(&entity)
    }

    /// How many live entities hold it.
    pub fn holders(&self) -> usize {
        self.index.len()
    }

    /// The component's default row.
    pub fn default_row(&self) -> Option<&ScriptValue> {
        self.default.as_ref()
    }

    /// Every holder, with its converted row — for comparing two projections.
    fn rows(&self) -> HashMap<EntityId, ScriptValue> {
        self.index
            .keys()
            .filter_map(|&entity| Some((entity, self.get(entity)?)))
            .collect()
    }
}

impl std::fmt::Debug for ProjectedComponent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectedComponent")
            .field("name", &self.name)
            .field("columns", &self.columns.len())
            .field("holders", &self.index.len())
            .finish()
    }
}

/// Every component the loaded programs read, projected for one frame.
#[derive(Default)]
pub struct ComponentProjection {
    components: Vec<ProjectedComponent>,
}

impl ComponentProjection {
    /// A projection of `components`.
    pub fn new(components: Vec<ProjectedComponent>) -> Self {
        Self { components }
    }

    /// The projected component named `name`.
    pub fn component(&self, name: &str) -> Option<&ProjectedComponent> {
        self.components
            .iter()
            .find(|component| component.name.as_str() == name)
    }

    /// The names of every projected component.
    pub fn names(&self) -> Vec<&ComponentName> {
        self.components
            .iter()
            .map(|component| &component.name)
            .collect()
    }

    /// How many rows the projection indexed, every component together.
    pub fn rows(&self) -> usize {
        self.components
            .iter()
            .map(ProjectedComponent::holders)
            .sum()
    }

    /// Whether nothing is projected.
    pub fn is_empty(&self) -> bool {
        self.components.is_empty()
    }
}

impl std::fmt::Debug for ComponentProjection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(&self.components).finish()
    }
}

impl PartialEq for ComponentProjection {
    /// By value: the same components, held by the same entities, with the
    /// same rows and defaults — however the snapshots are laid out.
    fn eq(&self, other: &Self) -> bool {
        self.components.len() == other.components.len()
            && self.components.iter().all(|mine| {
                other.component(mine.name.as_str()).is_some_and(|theirs| {
                    mine.default == theirs.default && mine.rows() == theirs.rows()
                })
            })
    }
}

/// The frame's working set: the projection, and what this frame's scripts
/// wrote over it.
#[derive(Debug, Default)]
pub struct FrameComponents {
    projection: Arc<ComponentProjection>,
    overlay: HashMap<(ComponentName, EntityId), Option<ScriptValue>>,
}

impl FrameComponents {
    /// The working set of a frame over `projection`, nothing written yet.
    pub fn new(projection: Arc<ComponentProjection>) -> Self {
        Self {
            projection,
            overlay: HashMap::new(),
        }
    }

    /// `entity`'s copy of component `name` — `None` when it does not hold it.
    pub fn get(&self, name: &str, entity: EntityId) -> Option<ScriptValue> {
        match self.overlay.get(&(ComponentName::new(name), entity)) {
            Some(written) => written.clone(),
            None => self.projection.component(name)?.get(entity),
        }
    }

    /// Whether `entity` holds component `name`.
    pub fn has(&self, name: &str, entity: EntityId) -> bool {
        match self.overlay.get(&(ComponentName::new(name), entity)) {
            Some(written) => written.is_some(),
            None => self
                .projection
                .component(name)
                .is_some_and(|component| component.has(entity)),
        }
    }

    /// Takes a command a script queued into the working set, as the frame
    /// boundary will apply it: a write changes the copy, an add attaches the
    /// defaults with the patch, a removal detaches. Only a projected component
    /// is tracked — no loaded program reads the others — and a command the
    /// boundary would refuse changes nothing here either.
    pub fn observe(&mut self, command: &WorldCommand) {
        match command {
            WorldCommand::SetComponent {
                entity,
                component,
                value,
            } => {
                let Some(current) = self
                    .tracked(component)
                    .and(self.get(component.as_str(), *entity))
                else {
                    return;
                };
                self.overlay
                    .insert((component.clone(), *entity), Some(merged(current, value)));
            }
            WorldCommand::AddComponent {
                entity,
                component,
                value,
            } => {
                let Some(projected) = self.tracked(component) else {
                    return;
                };
                if self.has(component.as_str(), *entity) {
                    return;
                }
                let start = projected
                    .default_row()
                    .cloned()
                    .unwrap_or(ScriptValue::Struct(Vec::new()));
                self.overlay
                    .insert((component.clone(), *entity), Some(merged(start, value)));
            }
            WorldCommand::RemoveComponent { entity, component }
                if self.tracked(component).is_some() && self.has(component.as_str(), *entity) =>
            {
                self.overlay.insert((component.clone(), *entity), None);
            }
            // Each row a `Set` of the batch's fields, in the batch's order.
            WorldCommand::WriteColumn { component, batch } => {
                if self.tracked(component).is_none() {
                    return;
                }
                for (entity, values) in batch.rows() {
                    let Some(current) = self.get(component.as_str(), entity) else {
                        continue;
                    };
                    let patch = ScriptValue::Struct(
                        batch
                            .fields()
                            .iter()
                            .cloned()
                            .zip(values.iter().cloned())
                            .collect(),
                    );
                    self.overlay
                        .insert((component.clone(), entity), Some(merged(current, &patch)));
                }
            }
            _ => {}
        }
    }

    /// The projected component `name`, when the working set tracks it.
    fn tracked(&self, name: &ComponentName) -> Option<&ProjectedComponent> {
        self.projection.component(name.as_str())
    }
}

/// `copy` with the fields `patch` names written over it, in the copy's own
/// order; a field the copy lacks is added after its own.
fn merged(copy: ScriptValue, patch: &ScriptValue) -> ScriptValue {
    let ScriptValue::Struct(mut fields) = copy else {
        return copy;
    };
    if let ScriptValue::Struct(written) = patch {
        for (name, value) in written {
            match fields.iter_mut().find(|(known, _)| known == name) {
                Some((_, slot)) => *slot = value.clone(),
                None => fields.push((name.clone(), value.clone())),
            }
        }
    }
    ScriptValue::Struct(fields)
}
