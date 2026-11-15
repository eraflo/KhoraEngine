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

//! What a save holds, by name: a [`Record`] is a value in serde's data model,
//! with entities and assets kept as references, not as a running world's ids.
//!
//! See the book: concepts/serialization.md, "Why records".

mod access;
mod blind;
mod de;
mod diff;
mod elements;
mod number;
mod one_variant;
mod report;
mod self_describing;
mod ser;
mod tree;
mod value;
mod watch;

pub use de::{from_record, ReferenceReader};
pub(crate) use diff::same;
pub use diff::{diff, patch};
pub use report::{resolve, unusable_entries, LoadReport, ReportEntry, ReportKind};
pub use ser::{to_record, ReferenceWriter};
pub use value::{EntityRef, Record, VariantPayload};

/// The serde name that marks an entity reference.
pub(crate) const ENTITY_NAME: &str = "khora.EntityId";

/// The serde name that marks an asset reference.
pub(crate) const ASSET_NAME: &str = "khora.AssetUUID";

/// How deep a value may nest.
///
/// A record comes from a file, which can claim any depth: this sits far above
/// any persisted value and far below what exhausts a thread's stack.
pub(crate) const MAX_DEPTH: usize = 128;

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

#[cfg(test)]
mod tests;
