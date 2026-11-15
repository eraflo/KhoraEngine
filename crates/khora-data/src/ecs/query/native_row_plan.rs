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

/// The per-row work a Native query needs beyond its driver row.
///
/// Decided once by `World::native_row_plan`: a `Without` or `Option` term may
/// name a component of another domain, stored in another page than the driver
/// row. The empty plan keeps the per-row path check-free.
#[derive(Debug, Default)]
pub(crate) struct NativeRowPlan {
    /// `Without` types registered outside the driver domain, with their domain,
    /// checked per row by `has_foreign_excluded`.
    pub(crate) foreign_without: Vec<(SemanticDomain, TypeId)>,
    /// Whether each row's item is joined with `fetch_from_world`, for an
    /// `Option` term outside the driver domain.
    ///
    /// That join checks every `Without` term too, so `foreign_without` stays empty.
    pub(crate) fetch_from_world: bool,
}
