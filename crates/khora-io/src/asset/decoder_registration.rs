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

//! The `DecoderRegistration` inventory entry: an asset decoder registered on
//! the `AssetService` at startup.

use super::AssetService;

/// Inventory entry for an `AssetDecoder` that wants to be auto-registered
/// against an [`AssetService`] at startup.
///
/// Mirrors the existing pattern used by `MaterialRegistration`
/// (`crates/khora-data/src/scene/material_registration.rs`),
/// `DataSystemRegistration`, and `FlowRegistration`.
///
/// Use this for decoder slots with **a single canonical implementation**
/// (e.g. `texture`, `font`). Slots where multiple backends compete (`audio`,
/// `mesh`) deliberately stay explicit at the call site — see the rationale
/// in `decoders/audio/mod.rs` and `decoders/mesh/mod.rs`.
///
/// ```ignore
/// inventory::submit! {
///     DecoderRegistration {
///         type_name: "texture",
///         register: |svc| {
///             svc.register_decoder::<CpuTexture>("texture", TextureDecoder);
///         },
///     }
/// }
/// ```
pub struct DecoderRegistration {
    /// Asset type name the decoder handles. Must match what
    /// `IndexBuilder::asset_type_for_extension` returns for the relevant
    /// file extensions (e.g. `"texture"`).
    pub type_name: &'static str,
    /// Function that performs the registration on an [`AssetService`].
    /// Plain function pointer — no captures — so the entry can be embedded
    /// in `inventory::submit!` (which needs `'static`).
    pub register: fn(&mut AssetService),
}

inventory::collect!(DecoderRegistration);
