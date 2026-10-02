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

//! What a save holds, by name.
//!
//! A [`Record`] is a value written down in serde's data model: structs with
//! their field names, enums with their variant names, and the two kinds of
//! reference the engine owns — entities and assets — as references rather than
//! as the numbers a running world happens to use for them.
//!
//! # Why serde's data model is the schema
//!
//! Every type the engine persists already derives serde, and serde already
//! describes it by name. Its attributes are the evolution vocabulary a save
//! needs: `#[serde(default)]` for a field the save predates, `#[serde(alias)]`
//! for one that was renamed. Reading a record back matches fields **by name**,
//! so a reordered struct reads the same, a new field takes its default and a
//! removed one is dropped — the save records the WHAT, and how a type is laid
//! out in memory or in a file is free to change.
//!
//! # References
//!
//! [`EntityId`] and [`AssetUUID`] carry reserved serde names
//! (`khora.EntityId`, `khora.AssetUUID`). The codec recognises them wherever
//! they sit — a field, a list, a script value, a frozen register — and hands
//! entities to a [`ReferenceWriter`] or [`ReferenceReader`], which translate
//! between a running world's ids and the identities a save keeps.
//!
//! [`EntityId`]: khora_core::ecs::entity::EntityId
//! [`AssetUUID`]: khora_core::asset::AssetUUID

mod access;
mod de;
mod number;
mod report;
mod ser;
mod value;
mod watch;

#[cfg(test)]
mod tests;

pub use de::{from_record, ReferenceReader};
pub use report::{resolve, LoadReport, ReportEntry, ReportKind};
pub use ser::{to_record, ReferenceWriter};
pub use value::{EntityRef, Record, VariantPayload};

/// The serde name that marks an entity reference.
const ENTITY_NAME: &str = "khora.EntityId";

/// The serde name that marks an asset reference.
const ASSET_NAME: &str = "khora.AssetUUID";

/// How deep a value may nest.
///
/// Far beyond any value the engine persists — a script's nested arrays are a
/// handful deep — and far below what exhausts a thread's stack. A record comes
/// from a file, and a file can claim any depth.
const MAX_DEPTH: usize = 128;

/// Why a value could not be written to, or read from, a record.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordError(pub String);

impl std::fmt::Display for RecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RecordError {}

impl serde::ser::Error for RecordError {
    fn custom<T: std::fmt::Display>(message: T) -> Self {
        Self(message.to_string())
    }
}

impl serde::de::Error for RecordError {
    fn custom<T: std::fmt::Display>(message: T) -> Self {
        Self(message.to_string())
    }
}
