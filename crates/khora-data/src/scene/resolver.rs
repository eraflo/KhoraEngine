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

//! What a load's references read as: the ids reserved for the entities the
//! record brings, and nowhere — reported — for any other.

use std::collections::HashMap;

use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;

use super::apply::Loaded;
use super::record::{EntityRef, RecordError, ReferenceReader, ReportEntry, ReportKind};
use crate::ecs::World;

/// Resolves the references a record holds to the ids reserved for this load.
pub(super) struct Resolver<'w, 'r> {
    pub(super) world: &'w mut World,
    pub(super) reserved: &'r HashMap<PersistentId, EntityId>,
    /// The entity and component being read, for report entries.
    pub(super) current: Option<(PersistentId, Loaded)>,
    /// The entity the last reference read was bound to, if the load brings
    /// it — what a `Parent` names.
    pub(super) bound: Option<EntityId>,
    pub(super) entries: Vec<ReportEntry>,
}

impl ReferenceReader for Resolver<'_, '_> {
    fn read_entity(&mut self, reference: EntityRef) -> Result<EntityId, RecordError> {
        if let EntityRef::Id(id) = reference {
            if let Some(entity) = self.reserved.get(&id) {
                self.bound = Some(*entity);
                return Ok(*entity);
            }
        }
        let (entity, component) = self
            .current
            .as_ref()
            .map(|(id, loaded)| (*id, loaded))
            .unzip();
        self.entries.push(ReportEntry {
            entity,
            component: component.map(|loaded| loaded.name().to_owned()),
            path: String::new(),
            kind: ReportKind::DeadReference,
        });
        Ok(self.world.nowhere())
    }
}
