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

//! Many rows of one component, written in one pass at the frame boundary.

use crate::ecs::entity::EntityId;

use super::ScriptValue;

/// Rows of one component and the fields every row writes.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnBatch {
    fields: Vec<String>,
    entities: Vec<EntityId>,
    values: Vec<ScriptValue>,
}

/// Why a batch, or a row of one, was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchShapeError {
    /// The batch names this field twice: a row would write it twice.
    DuplicateField(String),
    /// A row holds a number of values other than the batch's fields.
    WrongWidth {
        /// How many fields the batch writes.
        expected: usize,
        /// How many values the row held.
        found: usize,
    },
}

impl ColumnBatch {
    /// An empty batch writing `fields`, refusing one that names a field twice.
    pub fn new(fields: Vec<String>) -> Result<Self, BatchShapeError> {
        for (at, field) in fields.iter().enumerate() {
            if fields[..at].contains(field) {
                return Err(BatchShapeError::DuplicateField(field.clone()));
            }
        }
        Ok(Self {
            fields,
            entities: Vec::new(),
            values: Vec::new(),
        })
    }

    /// Appends `entity`'s row, refusing one whose width is not the batch's —
    /// the boundary never meets a ragged batch. A refused row leaves the
    /// batch as it was.
    pub fn push_row(
        &mut self,
        entity: EntityId,
        values: impl IntoIterator<Item = ScriptValue>,
    ) -> Result<(), BatchShapeError> {
        let start = self.values.len();
        self.values.extend(values);
        let found = self.values.len() - start;
        if found != self.fields.len() {
            self.values.truncate(start);
            return Err(BatchShapeError::WrongWidth {
                expected: self.fields.len(),
                found,
            });
        }
        self.entities.push(entity);
        Ok(())
    }

    /// The fields every row writes, in the order a row holds their values.
    pub fn fields(&self) -> &[String] {
        &self.fields
    }

    /// How many rows the batch holds.
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    /// Whether the batch holds no row.
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// Every row, in the order it was pushed: its entity and its values, one
    /// per field.
    pub fn rows(&self) -> impl Iterator<Item = (EntityId, &[ScriptValue])> + '_ {
        let width = self.fields.len();
        self.entities
            .iter()
            .enumerate()
            .map(move |(row, entity)| (*entity, &self.values[row * width..(row + 1) * width]))
    }
}
