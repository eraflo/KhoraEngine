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

//! The saved forms of a [`ProceduralMesh`]: its primitive and its parameters.

use khora_core::asset::{ProceduralMesh, ProceduralMeshKind};
use khora_core::renderer::api::gpu_scene::Mesh;
use serde::{Deserialize, Serialize};

use super::SavedLocalSource;
use crate::scene::positional::{from_positional, to_positional};
use crate::scene::record::{
    resolve, to_record, Record, RecordError, ReferenceReader, ReferenceWriter, ReportEntry,
    VariantPayload,
};
use crate::scene::schema::schema_of;

/// A procedural mesh as a file holds it: its uuid is derived again on load.
#[derive(Serialize, Deserialize)]
#[serde(rename = "ProceduralMesh")]
struct SavedProcedural {
    kind: ProceduralMeshKind,
    params: [f32; 4],
}

impl SavedProcedural {
    /// Returns the saved form of `mesh`.
    fn of(mesh: &ProceduralMesh) -> Self {
        Self {
            kind: mesh.kind(),
            params: mesh.params(),
        }
    }

    /// Returns the procedural mesh saved, named by its content again.
    fn into_source(self) -> ProceduralMesh {
        ProceduralMesh::new(self.kind, self.params)
    }
}

impl SavedLocalSource<Mesh> for ProceduralMesh {
    fn to_payload(
        &self,
        references: &mut dyn ReferenceWriter,
    ) -> Result<VariantPayload, RecordError> {
        match to_record(&SavedProcedural::of(self), references)? {
            Record::Struct { fields, .. } => Ok(VariantPayload::Struct(fields)),
            other => Err(RecordError(format!(
                "a procedural mesh is recorded as a struct, not {other:?}"
            ))),
        }
    }

    fn from_payload(
        payload: &Record,
        references: &mut dyn ReferenceReader,
    ) -> Result<(Self, Vec<ReportEntry>), RecordError> {
        let (saved, report) = resolve::<SavedProcedural>(payload, references)?;
        Ok((saved.into_source(), report))
    }

    fn to_positional(
        &self,
        out: &mut Vec<u8>,
        references: &mut dyn ReferenceWriter,
    ) -> Result<(), RecordError> {
        to_positional(&SavedProcedural::of(self), out, references)
    }

    fn from_positional(
        bytes: &[u8],
        references: &mut dyn ReferenceReader,
    ) -> Result<Self, RecordError> {
        from_positional::<SavedProcedural>(bytes, references).map(SavedProcedural::into_source)
    }

    fn schema() -> (u64, bool) {
        let schema = schema_of::<SavedProcedural>();
        (schema.fingerprint(), schema.is_complete())
    }

    fn to_json(&self) -> Option<serde_json::Value> {
        serde_json::to_value(SavedProcedural::of(self)).ok()
    }

    fn from_json(value: &serde_json::Value) -> Result<Self, String> {
        serde_json::from_value::<SavedProcedural>(value.clone())
            .map(SavedProcedural::into_source)
            .map_err(|error| error.to_string())
    }
}
