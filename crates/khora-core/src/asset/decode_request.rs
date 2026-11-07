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

//! A request to decode one asset off the frame.

use std::any::TypeId;

use crate::event::Supersedes;

use super::AssetUUID;

/// Decode the asset `uuid` as the Rust type `type_id`, asked for when the
/// asset service's change counter stood at `generation`.
///
/// Filed by the asset service on a cache miss, taken by the decode family's
/// lane. A decode that read a file changed since (invalidated, reindexed) is
/// never installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeRequest {
    /// The asset to decode.
    pub uuid: AssetUUID,
    /// The Rust type asked for (`TypeId::of::<A>()`).
    pub type_id: TypeId,
    /// The asset service's change counter when the request was filed.
    pub generation: u64,
}

impl Supersedes for DecodeRequest {}
