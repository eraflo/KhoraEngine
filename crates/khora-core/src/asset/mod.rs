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

//! Provides the foundational traits and primitive types for Khora's asset system.
//!
//! This module defines the "common language" for all asset-related operations in the
//! engine. It contains the core contracts that other crates will implement or use,
//! but it has no knowledge of how assets are loaded or stored.
//!
//! The key components are:
//! - The [`Stored`] trait: a marker for every type the asset system can store and hand out.
//! - The [`Asset`] trait: the kind of value an asset reference names.
//! - Stable, unique identifiers used to reference assets throughout the engine.
//! - Abstract interfaces for loading and managing assets.
//!
//! These low-level primitives are the foundation upon which higher-level systems,
//! such as an asset database or a virtual file system (VFS), are built in other
//! crates.

mod asset_decoding;
mod asset_indexing;
mod asset_kind;
mod asset_kind_registration;
mod asset_source;
mod asset_uuid;
mod compression_kind;
mod content_hasher;
mod decode_request;
mod decoded;
/// Font asset definitions and metadata.
pub mod font;
mod handle;
mod index_request;
mod indexed_assets;
/// The asset kinds the engine declares, one constant each.
pub mod kinds;
mod local_source;
mod materials;
mod metadata;
mod named_asset;
mod no_local_source;
mod procedural_mesh;
mod procedural_mesh_kind;
/// Ergon script modules, as the asset system holds them.
pub mod script;
mod stored;

pub use asset_decoding::AssetDecoding;
pub use asset_indexing::AssetIndexing;
pub use asset_kind::AssetKind;
pub use asset_kind_registration::AssetKindRegistration;
pub use asset_source::AssetSource;
pub use asset_uuid::*;
pub use compression_kind::CompressionKind;
pub use content_hasher::ContentHasher;
pub use decode_request::DecodeRequest;
pub use decoded::Decoded;
pub use handle::AssetHandle as Handle;
pub use handle::*;
pub use index_request::{IndexKind, IndexRequest, IndexTicket};
pub use indexed_assets::IndexedAssets;
pub use local_source::LocalSource;
pub use materials::*;
pub use metadata::*;
pub use named_asset::NamedAsset;
pub use no_local_source::NoLocalSource;
pub use procedural_mesh::ProceduralMesh;
pub use procedural_mesh_kind::ProceduralMeshKind;
pub use script::ScriptModule;
pub use stored::Stored;

/// A value an asset reference can name: its kind, the value a reference can
/// carry itself, and the assets it cannot be made without.
pub trait Asset: Stored + Sized {
    /// The kind this value is indexed, referenced and scripted as.
    const KIND: AssetKind;

    /// The value a reference carries itself, [`NoLocalSource`] for a kind no
    /// reference can carry.
    type LocalSource: LocalSource<Self>;

    /// Appends to `out` every asset this value cannot be made without.
    fn dependencies(&self, out: &mut Vec<NamedAsset>);
}
