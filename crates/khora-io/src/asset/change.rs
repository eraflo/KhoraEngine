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

//! An asset whose bytes changed, on its way to the copies the engine holds.

use khora_core::asset::AssetUUID;
use khora_core::event::Supersedes;

/// The asset `0` names changed: every copy of it the engine holds is stale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssetChanged(pub AssetUUID);

/// How many distinct unread changes the service's channel holds before the
/// oldest leaves to make room.
pub const CHANGE_BACKLOG: usize = 4096;

impl Supersedes for AssetChanged {
    const COALESCES: bool = true;

    /// Two changes of one asset before they are read are one change: the
    /// reader reloads the asset's current bytes either way.
    fn supersedes(&self, earlier: &Self) -> bool {
        self.0 == earlier.0
    }
}
