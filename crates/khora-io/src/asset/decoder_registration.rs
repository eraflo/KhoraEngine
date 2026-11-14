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

/// Inventory entry for an `AssetDecoder` registered on every [`AssetService`] at startup.
///
/// For a type with one canonical decoder (`texture`, `font`). A type whose
/// backends compete (`audio`, `mesh`) is registered at the call site instead;
/// `decoders/audio/mod.rs` and `decoders/mesh/mod.rs` say why.
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
    /// The asset type name the decoder handles (`"texture"`).
    ///
    /// Matches what `IndexBuilder::asset_type_for_extension` returns for its extensions.
    pub type_name: &'static str,
    /// Registers the decoder on an [`AssetService`].
    ///
    /// A plain function pointer, so the entry is `'static` for `inventory::submit!`.
    pub register: fn(&mut AssetService),
}

inventory::collect!(DecoderRegistration);
