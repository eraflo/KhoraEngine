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

//! The names the language gives asset kinds — `Prefab`, `Sound`, … — kept
//! for the asset types: no struct, component or behavior takes one.

use crate::ast::Item;
use crate::diagnostics::Span;

use super::Checker;

impl Checker {
    /// Whether `item` is a struct, a component or a behavior named like an
    /// asset kind — refused, and checked no further.
    pub(super) fn asset_named(item: &Item) -> bool {
        matches!(
            item,
            Item::Struct(_) | Item::Component(_) | Item::Behavior(_)
        ) && khora_core::asset::AssetKind::from_ergon_name(item.name()).is_some()
    }

    /// Refuses a struct, a component or a behavior named like an asset kind:
    /// the name would stand for two types, and the asset's could no longer be
    /// written.
    pub(super) fn refuse_asset_name(&mut self, name: &str, span: Span) {
        self.error_note(
            format!("`{name}` is a type of the language"),
            span,
            "it names a reference to an asset — `Prefab?`, `Sound?`, … — so no struct, component or behavior can take it",
        );
    }
}
