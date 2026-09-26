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

//! Serialising the world's pages to bytes and back (the Archetype strategy).

use std::{any::TypeId, collections::HashMap};

use bincode::config;

use super::World;
use crate::ecs::{page::ComponentPage, serialization::SceneMemoryLayout, SerializedPage};

/// Errors that can occur while reconstructing a `World` from a raw archetype
/// memory snapshot in [`World::deserialize_archetype`].
#[derive(Debug)]
pub enum DeserializeArchetypeError {
    /// The outer bincode payload could not be decoded.
    Decode(bincode::error::DecodeError),
    /// A serialized component type name is not present in the type registry of
    /// this `World`, so its column cannot be reconstructed.
    UnknownComponent(String),
    /// A column's raw bytes failed validation (misaligned or oversized length).
    InvalidColumn(crate::ecs::page::SetFromBytesError),
}

impl std::fmt::Display for DeserializeArchetypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeserializeArchetypeError::Decode(e) => write!(f, "archetype decode failed: {e}"),
            DeserializeArchetypeError::UnknownComponent(name) => {
                write!(f, "unknown serialized component type: {name}")
            }
            DeserializeArchetypeError::InvalidColumn(e) => {
                write!(f, "invalid component column: {e}")
            }
        }
    }
}

impl std::error::Error for DeserializeArchetypeError {}

impl From<bincode::error::DecodeError> for DeserializeArchetypeError {
    fn from(e: bincode::error::DecodeError) -> Self {
        DeserializeArchetypeError::Decode(e)
    }
}

impl World {
    /// Serializes the entire World state using a direct memory layout strategy.
    ///
    /// This method is highly unsafe as it reads raw component memory.
    pub fn serialize_archetype(&self) -> Result<Vec<u8>, bincode::error::EncodeError> {
        let mut serialized_pages = Vec::with_capacity(self.storage.pages.len());
        for page in &self.storage.pages {
            let mut serialized_columns = HashMap::new();

            // Use the TypeRegistry to get the stable string name for each TypeId.
            let type_names: Vec<String> = page
                .type_ids
                .iter()
                .map(|id| self.type_registry.get_name_of(id).unwrap().to_string())
                .collect();

            for type_id in &page.type_ids {
                let type_name = self.type_registry.get_name_of(type_id).unwrap();
                let column = &page.columns[type_id];
                // The column owns its byte format (AoS raw bytes, or field-major
                // for a field-SoA column) — round-tripped by `set_from_bytes`.
                serialized_columns.insert(type_name.to_string(), column.to_bytes());
            }

            serialized_pages.push(SerializedPage {
                type_names,
                entities: page.entities.clone(),
                columns: serialized_columns,
            });
        }
        let layout = SceneMemoryLayout {
            entities: self.entities.entities.clone(),
            freed_entities: self.entities.freed_entities.clone(),
            pages: serialized_pages,
        };
        bincode::encode_to_vec(layout, config::standard())
    }
}

impl World {
    /// Deserializes and completely replaces the World state from a memory layout.
    ///
    /// This method is highly unsafe as it writes raw bytes into component vectors.
    pub fn deserialize_archetype(&mut self, data: &[u8]) -> Result<(), DeserializeArchetypeError> {
        let (layout, _): (SceneMemoryLayout, _) =
            bincode::decode_from_slice(data, config::standard())?;

        self.entities.entities = layout.entities;
        self.entities.freed_entities = layout.freed_entities;
        self.storage.pages.clear();

        for serialized_page in layout.pages {
            // Use the TypeRegistry to convert string names back to TypeIds. A
            // name absent from the registry comes from an untrusted/foreign
            // scene, so fail gracefully instead of panicking.
            let type_ids: Vec<TypeId> = serialized_page
                .type_names
                .iter()
                .map(|name| {
                    self.type_registry
                        .get_id_of(name)
                        .ok_or_else(|| DeserializeArchetypeError::UnknownComponent(name.clone()))
                })
                .collect::<Result<_, _>>()?;

            let mut new_page = ComponentPage {
                type_ids,
                entities: serialized_page.entities,
                columns: HashMap::new(),
            };

            for (type_name, bytes) in &serialized_page.columns {
                let type_id = self.type_registry.get_id_of(type_name).ok_or_else(|| {
                    DeserializeArchetypeError::UnknownComponent(type_name.clone())
                })?;
                let constructor = self
                    .storage
                    .registry
                    .get_column_constructor(&type_id)
                    .ok_or_else(|| {
                        DeserializeArchetypeError::UnknownComponent(type_name.clone())
                    })?;
                let mut column = constructor();
                // SAFETY: `constructor` is the registered column factory for
                // `type_id`, so it produces a column whose element type matches
                // the bytes serialized for that same type. `set_from_bytes`
                // additionally validates the byte length before any allocation,
                // returning an error (propagated here) on a hostile or
                // mismatched length rather than aborting.
                unsafe {
                    column
                        .set_from_bytes(bytes)
                        .map_err(DeserializeArchetypeError::InvalidColumn)?;
                }
                new_page.columns.insert(type_id, column);
            }
            self.storage.pages.push(new_page);
        }

        // The entire World content was replaced by raw storage writes —
        // every domain may have changed, so invalidate all cached Views.
        self.bump_all_domain_epochs();

        Ok(())
    }
}
