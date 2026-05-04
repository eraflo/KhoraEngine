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

//! A serialization strategy that uses a stable, intermediate representation.
//!
//! Uses inventory-based component registration to handle all component types
//! automatically. Each component is serialized as a base64-encoded blob keyed
//! by type name in a human-readable RON structure.

use super::{DeserializationError, SerializationError, SerializationStrategy};
use crate::ecs::World;
use crate::scene::registry::ComponentRegistration;
use khora_core::ecs::entity::EntityId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A single component in the stable intermediate representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentDefinition {
    /// The component type name (e.g., "Transform", "Camera").
    pub type_name: String,
    /// Base64-encoded binary data (bincode-encoded SerializableX).
    pub data_base64: String,
}

/// An entity definition in the stable intermediate representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityDefinition {
    /// The original entity ID (for reference).
    pub id: EntityId,
    /// The components attached to this entity.
    pub components: Vec<ComponentDefinition>,
}

/// The full scene definition in stable intermediate representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneDefinition {
    /// All entities present in the scene, in stable definition form.
    pub entities: Vec<EntityDefinition>,
}

/// A serialization strategy that uses a stable, intermediate representation.
///
/// Iterates all registered components via inventory to serialize every
/// component on every entity. The output is human-readable RON with
/// base64-encoded binary component data.
#[derive(Default)]
pub struct DefinitionSerializationStrategy;

impl DefinitionSerializationStrategy {
    /// Creates a new `DefinitionSerializationStrategy`.
    pub fn new() -> Self {
        Self
    }
}

impl SerializationStrategy for DefinitionSerializationStrategy {
    fn get_strategy_id(&self) -> &'static str {
        "KH_DEFINITION_RON_V1"
    }

    fn serialize(&self, world: &World) -> Result<Vec<u8>, SerializationError> {
        let mut entity_defs = Vec::new();

        for entity_id in world.iter_entities() {
            let mut component_defs = Vec::new();

            // Iterate ALL registered components via inventory.
            for reg in inventory::iter::<ComponentRegistration> {
                if let Some(data) = (reg.serialize_recipe)(world, entity_id) {
                    component_defs.push(ComponentDefinition {
                        type_name: reg.type_name.to_string(),
                        data_base64: base64_encode(&data),
                    });
                }
            }

            if !component_defs.is_empty() {
                entity_defs.push(EntityDefinition {
                    id: entity_id,
                    components: component_defs,
                });
            }
        }

        let scene_definition = SceneDefinition {
            entities: entity_defs,
        };

        let pretty_config = ron::ser::PrettyConfig::default().indentor("  ".to_string());
        ron::ser::to_string_pretty(&scene_definition, pretty_config)
            .map(|s| s.into_bytes())
            .map_err(|e| SerializationError::ProcessingFailed(e.to_string()))
    }

    fn deserialize(&self, data: &[u8], world: &mut World) -> Result<(), DeserializationError> {
        let scene_def: SceneDefinition = ron::de::from_bytes(data)
            .map_err(|e| DeserializationError::InvalidFormat(e.to_string()))?;

        let mut id_map = HashMap::<EntityId, EntityId>::new();

        // First pass: spawn all entities.
        for entity_def in &scene_def.entities {
            let new_id = world.spawn(());
            id_map.insert(entity_def.id, new_id);
        }

        // Second pass: add components.
        for entity_def in &scene_def.entities {
            let new_id = id_map[&entity_def.id];

            for comp_def in &entity_def.components {
                let data = base64_decode(&comp_def.data_base64)
                    .map_err(DeserializationError::InvalidFormat)?;

                // Find the registration by type_name.
                for reg in inventory::iter::<ComponentRegistration> {
                    if reg.type_name == comp_def.type_name {
                        if let Err(e) = (reg.deserialize_recipe)(world, new_id, &data) {
                            log::warn!("Failed to deserialize {}: {}", comp_def.type_name, e);
                        }
                        break;
                    }
                }
            }
        }

        Ok(())
    }
}

// Simple base64 encoding/decoding (no external dependency needed for small blobs)
fn base64_encode(data: &[u8]) -> String {
    use std::fmt::Write;
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    let mut chunks = data.chunks_exact(3);
    for chunk in chunks.by_ref() {
        let n = ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8) | (chunk[2] as u32);
        write!(
            out,
            "{}{}{}{}",
            CHARS[((n >> 18) & 0x3F) as usize] as char,
            CHARS[((n >> 12) & 0x3F) as usize] as char,
            CHARS[((n >> 6) & 0x3F) as usize] as char,
            CHARS[(n & 0x3F) as usize] as char,
        )
        .unwrap();
    }
    let rem = chunks.remainder();
    if rem.len() == 1 {
        let n = (rem[0] as u32) << 16;
        write!(
            out,
            "{}{}==",
            CHARS[((n >> 18) & 0x3F) as usize] as char,
            CHARS[((n >> 12) & 0x3F) as usize] as char,
        )
        .unwrap();
    } else if rem.len() == 2 {
        let n = ((rem[0] as u32) << 16) | ((rem[1] as u32) << 8);
        write!(
            out,
            "{}{}{}=",
            CHARS[((n >> 18) & 0x3F) as usize] as char,
            CHARS[((n >> 12) & 0x3F) as usize] as char,
            CHARS[((n >> 6) & 0x3F) as usize] as char,
        )
        .unwrap();
    }
    out
}

fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    let input = input.trim_end_matches('=');
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0u8;
    for c in input.chars() {
        let val = match c {
            'A'..='Z' => c as u32 - 'A' as u32,
            'a'..='z' => c as u32 - 'a' as u32 + 26,
            '0'..='9' => c as u32 - '0' as u32 + 52,
            '+' => 62,
            '/' => 63,
            _ => return Err(format!("Invalid base64 character: {}", c)),
        };
        buf = (buf << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Ok(out)
}
