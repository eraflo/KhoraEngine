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

//! The registrations, made through `inventory`, by which any material type, a
//! plugin's included, is saved and read back by its type name.

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

/// Writes a `dyn Material` by position, its references through the writer.
pub type MaterialToPositionalFn =
    fn(&dyn Material, &mut Vec<u8>, &mut dyn ReferenceWriter) -> Option<Result<(), RecordError>>;

/// Reads a material back from its positional bytes.
pub type MaterialFromPositionalFn =
    fn(&[u8], &mut dyn ReferenceReader) -> Result<Box<dyn Material>, RecordError>;

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
    /// Writes a `dyn Material` by position, as a snapshot holds it. `None`
    /// if the material is not this registration's concrete type.
    pub to_positional: MaterialToPositionalFn,
    /// Reads a material back from its positional bytes, consumed exactly.
    pub from_positional: MaterialFromPositionalFn,
    /// The fingerprint of the concrete type's schema.
    pub schema: fn() -> u64,
    /// Whether that schema was traced in full.
    pub schema_complete: fn() -> bool,
    /// Creates a default instance of this material type (for placeholder handles).
    pub create_default: fn() -> Box<dyn Material>,
    /// Writes a `dyn Material` as editable JSON, `None` if it is not this registration's type.
    pub serialize_json: fn(&dyn Material) -> Option<serde_json::Value>,
    /// Deserializes a serde-JSON value (as produced by `serialize_json`) back
    /// into a `Box<dyn Material>`.
    pub deserialize_json: fn(&serde_json::Value) -> Result<Box<dyn Material>, String>,
}

collect!(MaterialRegistration);

/// Writes a material down: its type name and its record.
///
/// No registration claiming the material is not a reason to lose it: it is
/// written as a standard material of the same base color, the one thing every
/// material has.
///
/// # Errors
///
/// When the material, or its fallback, cannot be written as a record.
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

/// Reads a material back from its type name and record.
///
/// # Errors
///
/// When no registered material type is named `type_name`, or the record does not
/// read as that type.
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

/// Implements `to_record` for a concrete material type.
fn concrete_to_record<M: Material + Serialize + 'static>(
    material: &dyn Material,
    references: &mut dyn ReferenceWriter,
) -> Option<Result<Record, RecordError>> {
    material
        .as_any()
        .downcast_ref::<M>()
        .map(|material| to_record(material, references))
}

/// Implements `to_positional` for a concrete material type.
fn concrete_to_positional<M: Material + Serialize + 'static>(
    material: &dyn Material,
    out: &mut Vec<u8>,
    references: &mut dyn ReferenceWriter,
) -> Option<Result<(), RecordError>> {
    material
        .as_any()
        .downcast_ref::<M>()
        .map(|material| super::positional::to_positional(material, out, references))
}

/// Implements `from_positional` for a concrete material type.
fn concrete_from_positional<M: Material + DeserializeOwned + 'static>(
    bytes: &[u8],
    references: &mut dyn ReferenceReader,
) -> Result<Box<dyn Material>, RecordError> {
    super::positional::from_positional::<M>(bytes, references)
        .map(|material| Box::new(material) as Box<dyn Material>)
}

/// Implements `schema` and `schema_complete` for a concrete material type.
fn concrete_schema<M: DeserializeOwned>() -> (u64, bool) {
    let schema = super::schema::schema_of::<M>();
    (schema.fingerprint(), schema.is_complete())
}

/// Writes a material by position: its type name, then its bytes — what
/// its own registration wrote.
///
/// No registration claiming the material falls back as the record does: a
/// standard material of the same base color.
///
/// # Errors
///
/// When the material, or its fallback, cannot be written by position.
pub(crate) fn material_to_positional(
    material: &dyn Material,
    references: &mut dyn ReferenceWriter,
) -> Result<(&'static str, Vec<u8>), RecordError> {
    let mut bytes = Vec::new();
    for reg in inventory::iter::<MaterialRegistration> {
        if let Some(written) = (reg.to_positional)(material, &mut bytes, references) {
            return written.map(|()| (reg.type_name, bytes));
        }
    }
    let fallback = StandardMaterial {
        base_color: material.base_color(),
        ..StandardMaterial::default()
    };
    super::positional::to_positional(&fallback, &mut bytes, references)?;
    Ok(("StandardMaterial", bytes))
}

/// Returns the material `type_name` names, read from its positional bytes.
///
/// # Errors
///
/// When no registered material type is named `type_name`, or the bytes do not
/// read as that type, consumed exactly.
pub(crate) fn material_from_positional(
    type_name: &str,
    bytes: &[u8],
    references: &mut dyn ReferenceReader,
) -> Result<Box<dyn Material>, RecordError> {
    let reg = inventory::iter::<MaterialRegistration>
        .into_iter()
        .find(|reg| reg.type_name == type_name)
        .ok_or_else(|| RecordError(format!("no material type `{type_name}`")))?;
    (reg.from_positional)(bytes, references)
}

/// Returns every registered material's schema, sorted by name, and whether
/// each was traced in full: what a snapshot holding a material is read by.
pub(crate) fn materials_schema() -> (String, bool) {
    let mut materials: Vec<(&str, u64, bool)> = inventory::iter::<MaterialRegistration>
        .into_iter()
        .map(|reg| (reg.type_name, (reg.schema)(), (reg.schema_complete)()))
        .collect();
    materials.sort_unstable();
    let complete = materials.iter().all(|(_, _, complete)| *complete);
    let text = materials
        .iter()
        .map(|(name, fingerprint, _)| format!("{name}={fingerprint:016x}"))
        .collect::<Vec<_>>()
        .join(",");
    (text, complete)
}

/// Implements `stage` for a concrete material type.
fn concrete_stage<M: Material + Serialize + DeserializeOwned + 'static>(
    record: &Record,
    references: &mut dyn ReferenceReader,
) -> Result<(Box<dyn Material>, Vec<ReportEntry>), RecordError> {
    let (material, report) = resolve::<M>(record, references)?;
    Ok((Box::new(material) as Box<dyn Material>, report))
}

/// Returns a material as editable JSON: `{ "type_name": <name>, "material": <material> }`.
///
/// `None` if no registration claims the material.
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

/// Returns a new handle and id for the material [`material_to_json`] wrote.
///
/// # Errors
///
/// When the JSON lacks a part, names no registered type, or does not decode.
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

// The built-in materials: what `MaterialRef`'s registration and the `.kmat`
// decoder dispatch through by type name.

inventory::submit! {
    MaterialRegistration {
        type_name: "StandardMaterial",
        to_record: concrete_to_record::<StandardMaterial>,
        stage: concrete_stage::<StandardMaterial>,
        to_positional: concrete_to_positional::<StandardMaterial>,
        from_positional: concrete_from_positional::<StandardMaterial>,
        schema: || concrete_schema::<StandardMaterial>().0,
        schema_complete: || concrete_schema::<StandardMaterial>().1,
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
        to_positional: concrete_to_positional::<UnlitMaterial>,
        from_positional: concrete_from_positional::<UnlitMaterial>,
        schema: || concrete_schema::<UnlitMaterial>().0,
        schema_complete: || concrete_schema::<UnlitMaterial>().1,
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
        to_positional: concrete_to_positional::<EmissiveMaterial>,
        from_positional: concrete_from_positional::<EmissiveMaterial>,
        schema: || concrete_schema::<EmissiveMaterial>().0,
        schema_complete: || concrete_schema::<EmissiveMaterial>().1,
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
        to_positional: concrete_to_positional::<WireframeMaterial>,
        from_positional: concrete_from_positional::<WireframeMaterial>,
        schema: || concrete_schema::<WireframeMaterial>().0,
        schema_complete: || concrete_schema::<WireframeMaterial>().1,
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
