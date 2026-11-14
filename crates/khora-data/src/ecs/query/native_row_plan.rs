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

//! The `NativeRowPlan`: the per-row work a Native query does beyond its
//! driver row.

use khora_core::ecs::SemanticDomain;
use std::any::TypeId;

/// The per-row work a Native query needs beyond reading its driver row, decided
/// once at query construction by `World::native_row_plan`.
///
/// A Native query iterates the pages of its driver domain, but a `Without` or
/// `Option` term may name a component registered in another domain, stored in
/// another page than the driver row. The default (empty) plan — every term in
/// the driver domain — keeps the per-row path check-free.
#[derive(Debug, Default)]
pub(crate) struct NativeRowPlan {
    /// `Without` types registered outside the driver domain, with their domain,
    /// checked per row by `has_foreign_excluded`.
    pub(crate) foreign_without: Vec<(SemanticDomain, TypeId)>,
    /// An `Option` term names a type registered outside the driver domain: the
    /// page-row `fetch` would read it from the driver page, which never holds
    /// it, so each live row's item is joined through the entity's live
    /// locations with `fetch_from_world` instead (which also checks every
    /// `Without` term, so `foreign_without` is left empty).
    pub(crate) fetch_from_world: bool,
}
