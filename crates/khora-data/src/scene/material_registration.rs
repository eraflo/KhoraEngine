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
//! a type name and how it is written to, and read from, a record and JSON.
//! Any custom material (including those from plugins) is saved as long as it
//! registers itself, with `inventory::submit!`.

use inventory::collect;
use khora_core::asset::{AssetHandle, AssetUUID, Material, StandardMaterial};
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::record::{
    resolve, to_record, Record, RecordError, ReferenceReader, ReferenceWriter, ReportEntry,
};

/// Writes a `dyn Material` as a record, its references through the writer.
pub type MaterialToRecordFn =
    fn(&dyn Material, &mut dyn ReferenceWriter) -> Option<Result<Record, RecordError>>;

/// Reads a material back from a record, and says what the read adapted.
pub type MaterialStageFn = fn(
    &Record,
    &mut dyn ReferenceReader,
) -> Result<(Box<dyn Material>, Vec<ReportEntry>), RecordError>;

/// Registration entry for a serializable material type.
///
/// Each material type submits one via `inventory::submit!`.
pub struct MaterialRegistration {
    /// Unique type name: what a save and a `.kmat` file name the material by.
    pub type_name: &'static str,
    /// Writes a `dyn Material` as a record. `None` if the material is not
    /// this registration's concrete type.
    pub to_record: MaterialToRecordFn,
    /// Reads a material back from a record.
    pub stage: MaterialStageFn,
    /// Creates a default instance of this material type (for placeholder handles).
    pub create_default: fn() -> Box<dyn Material>,
    /// Serializes a `dyn Material` into a serde-JSON value for the editor
    /// inspector. Returns `None` if the material does not match this
    /// registration's concrete type.
    pub serialize_json: fn(&dyn Material) -> Option<serde_json::Value>,
    /// Deserializes a serde-JSON value (as produced by `serialize_json`) back
    /// into a `Box<dyn Material>`.
    pub deserialize_json: fn(&serde_json::Value) -> Result<Box<dyn Material>, String>,
}

collect!(MaterialRegistration);

/// A material written down: its type name and its record.
///
/// No registration claiming the material is not a reason to lose it: it is
/// written as a standard material of the same base color, the one thing every
/// material has.
pub fn material_to_record(
    material: &dyn Material,
    references: &mut dyn ReferenceWriter,
) -> Result<(&'static str, Record), RecordError> {
    for reg in inventory::iter::<MaterialRegistration> {
        if let Some(record) = (reg.to_record)(material, references) {
            return Ok((reg.type_name, record?));
        }
    }
    log::warn!(
        "no MaterialRegistration claims a material; saving its base color as a StandardMaterial"
    );
    let fallback = StandardMaterial {
        base_color: material.base_color(),
        ..Default::default()
    };
    Ok(("StandardMaterial", to_record(&fallback, references)?))
}

/// A material read back from its type name and record.
pub fn material_from_record(
    type_name: &str,
    record: &Record,
    references: &mut dyn ReferenceReader,
) -> Result<(Box<dyn Material>, Vec<ReportEntry>), RecordError> {
    inventory::iter::<MaterialRegistration>
        .into_iter()
        .find(|reg| reg.type_name == type_name)
        .ok_or_else(|| RecordError(format!("no material type `{type_name}`")))
        .and_then(|reg| (reg.stage)(record, references))
}

/// `to_record` for a concrete material type.
fn concrete_to_record<M: Material + Serialize + 'static>(
    material: &dyn Material,
    references: &mut dyn ReferenceWriter,
) -> Option<Result<Record, RecordError>> {
    material
        .as_any()
        .downcast_ref::<M>()
        .map(|material| to_record(material, references))
}

/// `stage` for a concrete material type.
fn concrete_stage<M: Material + Serialize + DeserializeOwned + 'static>(
    record: &Record,
    references: &mut dyn ReferenceReader,
) -> Result<(Box<dyn Material>, Vec<ReportEntry>), RecordError> {
    let (material, report) = resolve::<M>(record, references)?;
    Ok((Box::new(material) as Box<dyn Material>, report))
}

/// Serializes a material into an editable serde-JSON object of the form
/// `{ "type_name": <name>, "material": <concrete material> }` — the same
/// `(type_name, material)` split a saved record uses. Returns `None` if no
/// registration claims the material.
pub fn material_to_json(material: &dyn Material) -> Option<serde_json::Value> {
    for reg in inventory::iter::<MaterialRegistration> {
        if let Some(material_json) = (reg.serialize_json)(material) {
            return Some(serde_json::json!({
                "type_name": reg.type_name,
                "material": material_json,
            }));
        }
    }
    None
}

/// Reconstructs a `(handle, uuid)` pair from a JSON object produced by
/// [`material_to_json`]. The `type_name` selects the matching registration; the
/// `material` sub-value is decoded by that registration's `deserialize_json`.
pub fn material_from_json(
    value: &serde_json::Value,
) -> Result<(AssetHandle<Box<dyn Material>>, AssetUUID), String> {
    let type_name = value
        .get("type_name")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "material JSON missing string 'type_name'".to_string())?;
    let material_value = value
        .get("material")
        .ok_or_else(|| "material JSON missing 'material' object".to_string())?;

    for reg in inventory::iter::<MaterialRegistration> {
        if reg.type_name == type_name {
            let material = (reg.deserialize_json)(material_value)?;
            return Ok((AssetHandle::new(material), AssetUUID::new()));
        }
    }

    Err(format!(
        "No MaterialRegistration found for type '{type_name}'"
    ))
}

// ─── Built-in material registrations ───

use khora_core::asset::{EmissiveMaterial, UnlitMaterial, WireframeMaterial};

// The scene + inspector `ComponentRegistration` for the authored material
// reference lives on `MaterialRef` (see `material_ref.rs`); it reuses the
// helpers above. The four built-in `MaterialRegistration` entries below are
// the open type-tag registry those helpers (and the `.kmat` decoder) dispatch
// through — keep them.

inventory::submit! {
    MaterialRegistration {
        type_name: "StandardMaterial",
        to_record: concrete_to_record::<StandardMaterial>,
        stage: concrete_stage::<StandardMaterial>,
        create_default: || Box::new(StandardMaterial::default()) as Box<dyn Material>,
        serialize_json: |mat| {
            mat.as_any()
                .downcast_ref::<StandardMaterial>()
                .and_then(|m| serde_json::to_value(m).ok())
        },
        deserialize_json: |value| {
            let m: StandardMaterial =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            Ok(Box::new(m) as Box<dyn Material>)
        },
    }
}

inventory::submit! {
    MaterialRegistration {
        type_name: "UnlitMaterial",
        to_record: concrete_to_record::<UnlitMaterial>,
        stage: concrete_stage::<UnlitMaterial>,
        create_default: || Box::new(UnlitMaterial::default()) as Box<dyn Material>,
        serialize_json: |mat| {
            mat.as_any()
                .downcast_ref::<UnlitMaterial>()
                .and_then(|m| serde_json::to_value(m).ok())
        },
        deserialize_json: |value| {
            let m: UnlitMaterial =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            Ok(Box::new(m) as Box<dyn Material>)
        },
    }
}

inventory::submit! {
    MaterialRegistration {
        type_name: "EmissiveMaterial",
        to_record: concrete_to_record::<EmissiveMaterial>,
        stage: concrete_stage::<EmissiveMaterial>,
        create_default: || Box::new(EmissiveMaterial::default()) as Box<dyn Material>,
        serialize_json: |mat| {
            mat.as_any()
                .downcast_ref::<EmissiveMaterial>()
                .and_then(|m| serde_json::to_value(m).ok())
        },
        deserialize_json: |value| {
            let m: EmissiveMaterial =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            Ok(Box::new(m) as Box<dyn Material>)
        },
    }
}

inventory::submit! {
    MaterialRegistration {
        type_name: "WireframeMaterial",
        to_record: concrete_to_record::<WireframeMaterial>,
        stage: concrete_stage::<WireframeMaterial>,
        create_default: || Box::new(WireframeMaterial::default()) as Box<dyn Material>,
        serialize_json: |mat| {
            mat.as_any()
                .downcast_ref::<WireframeMaterial>()
                .and_then(|m| serde_json::to_value(m).ok())
        },
        deserialize_json: |value| {
            let m: WireframeMaterial =
                serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
            Ok(Box::new(m) as Box<dyn Material>)
        },
    }
}
