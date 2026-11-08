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

//! Who indexes projects for the `loading` family.

use crate::lane::Lease;

use super::{IndexRequest, IndexedAssets};

/// Indexes the project an [`IndexRequest`] names, off the frame, from the
/// `loading` family's lanes: it touches no `World` and no asset service.
pub trait AssetIndexing: Send + Sync {
    /// Indexes `request`'s project, and says what the result holds to: a
    /// [`Lease`] that stops holding once the project it was filed for is
    /// no longer the one open. Never panics: a failure is an `Err` in
    /// [`IndexedAssets::index`].
    fn index_leased(&self, request: &IndexRequest) -> (IndexedAssets, Option<Lease>);
}
