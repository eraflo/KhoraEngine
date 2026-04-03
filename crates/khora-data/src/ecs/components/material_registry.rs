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

//! Open, inventory-based registration system for serializable materials.
//!
//! Each concrete material type registers itself via `inventory::submit!` with
//! a type name, a serialize function, and a deserialize function. This allows
//! any custom material (including those from plugins) to be serializable as
//! long as it registers itself.
//!
//! The `#[derive(Material)]` proc-macro auto-generates the registration.

use bincode::config;
use inventory::collect;
use khora_core::asset::{AssetHandle, AssetUUID, Material};
use khora_core::math::LinearRgba;

/// A serializable wrapper for a material that stores the type name and binary data.
#[derive(bincode::Encode, bincode::Decode, Debug, Clone)]
pub struct SerializableMaterialData {
    pub type_name: String,
    pub data: Vec<u8>,
}

/// Registration entry for a serializable material type.
///
/// Submitted via `inventory::submit!` either manually or through `#[derive(Material)]`.
pub struct MaterialRegistration {
    /// Unique type name used for lookup during deserialization.
    pub type_name: &'static str,
    /// Serializes a `dyn Material` into binary data. Returns `None` if the material
    /// does not match this registration's concrete type.
    pub serialize: fn(&dyn Material) -> Option<Vec<u8>>,
    /// Deserializes binary data back into a `Box<dyn Material>`.
    pub deserialize: fn(&[u8]) -> Result<Box<dyn Material>, String>,
    /// Creates a default instance of this material type (for placeholder handles).
    pub create_default: fn() -> Box<dyn Material>,
}

collect!(MaterialRegistration);

/// Serializes a `MaterialComponent` by finding the matching `MaterialRegistration`
/// and encoding the material data with its type name.
pub fn serialize_material_component(
    base_color: LinearRgba,
    material: &dyn Material,
) -> Option<Vec<u8>> {
    for reg in inventory::iter::<MaterialRegistration> {
        if let Some(data) = (reg.serialize)(material) {
            let serializable = SerializableMaterialData {
                type_name: reg.type_name.to_string(),
                data,
            };
            return Some(bincode::encode_to_vec(&serializable, config::standard()).ok()?);
        }
    }
    // Fallback: no registration found, serialize just the base color as a StandardMaterial-like placeholder.
    log::warn!(
        "No MaterialRegistration found for material type; falling back to base-color-only serialization."
    );
    let serializable = SerializableMaterialData {
        type_name: "__unknown__".to_string(),
        data: bincode::encode_to_vec(&base_color, config::standard()).ok()?,
    };
    Some(bincode::encode_to_vec(&serializable, config::standard()).ok()?)
}

/// Deserializes a `MaterialComponent` from binary data.
pub fn deserialize_material_component(
    data: &[u8],
) -> Result<(AssetHandle<Box<dyn Material>>, AssetUUID), String> {
    let (serializable, _): (SerializableMaterialData, _) =
        bincode::decode_from_slice(data, config::standard()).map_err(|e| e.to_string())?;

    if serializable.type_name == "__unknown__" {
        // Reconstruct a basic StandardMaterial from the fallback base color.
        let (base_color, _): (LinearRgba, _) =
            bincode::decode_from_slice(&serializable.data, config::standard())
                .map_err(|e| e.to_string())?;
        let mat = khora_core::asset::StandardMaterial {
            base_color,
            ..Default::default()
        };
        let handle = AssetHandle::new(Box::new(mat) as Box<dyn Material>);
        return Ok((handle, AssetUUID::new()));
    }

    for reg in inventory::iter::<MaterialRegistration> {
        if reg.type_name == serializable.type_name {
            let material = (reg.deserialize)(&serializable.data)?;
            let uuid = AssetUUID::new();
            let handle = AssetHandle::new(material);
            return Ok((handle, uuid));
        }
    }

    Err(format!(
        "No MaterialRegistration found for type '{}'",
        serializable.type_name
    ))
}

// ─── Built-in material registrations ───

use khora_core::asset::{EmissiveMaterial, StandardMaterial, UnlitMaterial, WireframeMaterial};

inventory::submit! {
    MaterialRegistration {
        type_name: "StandardMaterial",
        serialize: |mat| {
            mat.as_any().downcast_ref::<StandardMaterial>().map(|m| {
                bincode::encode_to_vec(m, config::standard()).unwrap_or_default()
            })
        },
        deserialize: |data| {
            let (m, _) = bincode::decode_from_slice::<StandardMaterial, _>(data, config::standard())
                .map_err(|e| e.to_string())?;
            Ok(Box::new(m) as Box<dyn Material>)
        },
        create_default: || Box::new(StandardMaterial::default()) as Box<dyn Material>,
    }
}

inventory::submit! {
    MaterialRegistration {
        type_name: "UnlitMaterial",
        serialize: |mat| {
            mat.as_any().downcast_ref::<UnlitMaterial>().map(|m| {
                bincode::encode_to_vec(m, config::standard()).unwrap_or_default()
            })
        },
        deserialize: |data| {
            let (m, _) = bincode::decode_from_slice::<UnlitMaterial, _>(data, config::standard())
                .map_err(|e| e.to_string())?;
            Ok(Box::new(m) as Box<dyn Material>)
        },
        create_default: || Box::new(UnlitMaterial::default()) as Box<dyn Material>,
    }
}

inventory::submit! {
    MaterialRegistration {
        type_name: "EmissiveMaterial",
        serialize: |mat| {
            mat.as_any().downcast_ref::<EmissiveMaterial>().map(|m| {
                bincode::encode_to_vec(m, config::standard()).unwrap_or_default()
            })
        },
        deserialize: |data| {
            let (m, _) = bincode::decode_from_slice::<EmissiveMaterial, _>(data, config::standard())
                .map_err(|e| e.to_string())?;
            Ok(Box::new(m) as Box<dyn Material>)
        },
        create_default: || Box::new(EmissiveMaterial::default()) as Box<dyn Material>,
    }
}

inventory::submit! {
    MaterialRegistration {
        type_name: "WireframeMaterial",
        serialize: |mat| {
            mat.as_any().downcast_ref::<WireframeMaterial>().map(|m| {
                bincode::encode_to_vec(m, config::standard()).unwrap_or_default()
            })
        },
        deserialize: |data| {
            let (m, _) = bincode::decode_from_slice::<WireframeMaterial, _>(data, config::standard())
                .map_err(|e| e.to_string())?;
            Ok(Box::new(m) as Box<dyn Material>)
        },
        create_default: || Box::new(WireframeMaterial::default()) as Box<dyn Material>,
    }
}
