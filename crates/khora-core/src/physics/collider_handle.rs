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

//! Opaque handle to a collider in a physics backend.

use serde::{Deserialize, Serialize};

use super::handle::SlotId;

/// Opaque handle to a collider in the physics engine.
///
/// Carries the slot and its generation, for the reason
/// [`RigidBodyHandle`](super::RigidBodyHandle) gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ColliderHandle(pub u64);

impl ColliderHandle {
    /// The slot and generation this addresses.
    pub const fn slot(self) -> SlotId {
        SlotId::unpack(self.0)
    }
}
