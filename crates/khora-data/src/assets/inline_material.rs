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

//! The saved forms of an [`InlineMaterial`]: its material's type name and fields.

use khora_core::asset::{InlineMaterial, Material};
use serde::{Deserialize, Serialize};

use super::SavedLocalSource;
use crate::scene::positional::{from_positional, to_positional};
use crate::scene::record::{
    Record, RecordError, ReferenceReader, ReferenceWriter, ReportEntry, VariantPayload,
};
use crate::scene::schema::{fingerprint_of, schema_of};
use crate::scene::{
    material_from_json, material_from_positional, material_from_record, material_to_json,
    material_to_positional, material_to_record, materials_schema,
};

/// The field naming the material's registered type.
const TYPE_NAME: &str = "type_name";

/// The field holding the material itself.
const MATERIAL: &str = "material";

/// An inline material as a snapshot holds it: its type name and the bytes
/// its own registration wrote.
#[derive(Serialize, Deserialize)]
#[serde(rename = "InlineMaterial")]
struct InlineSnapshot {
    type_name: String,
    material: Vec<u8>,
}

impl SavedLocalSource<Box<dyn Material>> for InlineMaterial {
    // The type is named beside the fields: a self-describing format keeps
    // field names, not struct names, and the type picks the registration
    // that reads it back.
    fn to_payload(
        &self,
        references: &mut dyn ReferenceWriter,
    ) -> Result<VariantPayload, RecordError> {
        let (type_name, material) = material_to_record(self.material(), references)?;
        Ok(VariantPayload::Struct(vec![
            (TYPE_NAME.to_owned(), Record::Str(type_name.to_owned())),
            (MATERIAL.to_owned(), material),
        ]))
    }

    fn from_payload(
        payload: &Record,
        references: &mut dyn ReferenceReader,
    ) -> Result<(Self, Vec<ReportEntry>), RecordError> {
        let Some(Record::Str(type_name)) = field(payload, TYPE_NAME) else {
            return Err(RecordError(
                "an inline material without its type name".to_owned(),
            ));
        };
        let material = field(payload, MATERIAL)
            .ok_or_else(|| RecordError("an inline material without its value".to_owned()))?;
        let (material, mut report) = material_from_record(type_name, material, references)?;
        for entry in &mut report {
            entry.path = if entry.path.is_empty() {
                MATERIAL.to_owned()
            } else {
                format!("{MATERIAL}.{}", entry.path)
            };
        }
        Ok((InlineMaterial::new(material), report))
    }

    fn to_positional(
        &self,
        out: &mut Vec<u8>,
        references: &mut dyn ReferenceWriter,
    ) -> Result<(), RecordError> {
        let (type_name, material) = material_to_positional(self.material(), references)?;
        let snapshot = InlineSnapshot {
            type_name: type_name.to_owned(),
            material,
        };
        to_positional(&snapshot, out, references)
    }

    fn from_positional(
        bytes: &[u8],
        references: &mut dyn ReferenceReader,
    ) -> Result<Self, RecordError> {
        let snapshot = from_positional::<InlineSnapshot>(bytes, references)?;
        material_from_positional(&snapshot.type_name, &snapshot.material, references)
            .map(InlineMaterial::new)
    }

    /// Covers every registered material's shape, since an inline one may be any of them.
    fn schema() -> (u64, bool) {
        let shape = schema_of::<InlineSnapshot>();
        let (materials, materials_complete) = materials_schema();
        (
            fingerprint_of(&format!("{shape};{materials}")),
            shape.is_complete() && materials_complete,
        )
    }

    fn to_json(&self) -> Option<serde_json::Value> {
        material_to_json(self.material())
    }

    fn from_json(value: &serde_json::Value) -> Result<Self, String> {
        material_from_json(value).map(InlineMaterial::new)
    }
}

/// Returns the field `name` of a payload, as written or as a self-describing format reads it.
fn field<'a>(payload: &'a Record, name: &str) -> Option<&'a Record> {
    match payload {
        Record::Struct { fields, .. } => fields
            .iter()
            .find_map(|(key, value)| (key == name).then_some(value)),
        Record::Map(entries) => entries.iter().find_map(|(key, value)| match key {
            Record::Str(key) if key == name => Some(value),
            _ => None,
        }),
        _ => None,
    }
}
