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

//! The registration that declares an asset kind, and the macro that submits it.

use super::AssetKind;

/// One asset kind the engine knows: its name and, if a script can hold one, its Ergon name.
///
/// Submitted by [`register_asset_kind!`](crate::register_asset_kind); two
/// registrations of one name are refused.
#[derive(Debug, Clone, Copy)]
pub struct AssetKindRegistration {
    /// The kind declared.
    pub kind: AssetKind,
    /// The type Ergon spells the kind with: `"Mesh"`; `None` for a kind no script holds.
    pub ergon_name: Option<&'static str>,
}

inventory::collect!(AssetKindRegistration);

/// Declares an asset kind: a constant naming it and its registration.
///
/// ```ignore
/// khora_core::register_asset_kind!(
///     /// A loot table, `LootTable` in Ergon.
///     pub const LOOT_TABLE = "loot_table", ergon = "LootTable"
/// );
/// ```
///
/// Without `ergon = …`, no script can hold the kind.
#[macro_export]
macro_rules! register_asset_kind {
    (@ergon) => {
        ::core::option::Option::None
    };
    (@ergon $ergon:literal) => {
        ::core::option::Option::Some($ergon)
    };
    (
        $(#[$meta:meta])*
        $vis:vis const $constant:ident = $name:literal $(, ergon = $ergon:literal)? $(,)?
    ) => {
        $(#[$meta])*
        $vis const $constant: $crate::asset::AssetKind =
            $crate::asset::AssetKind::__declare($name);
        $crate::__inventory::submit! {
            $crate::asset::AssetKindRegistration {
                kind: $constant,
                ergon_name: $crate::register_asset_kind!(@ergon $($ergon)?),
            }
        }
    };
}
