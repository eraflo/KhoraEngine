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

//! What one physics step reported, on its way from the lane to the channel.

use super::collision::Collision;

/// What one physics step reported, on its way from the lane to the channel.
///
/// A deck slot, because that is the sanctioned road out of a lane: a lane that
/// wrote the shared channel directly would be a lane reaching engine state, and
/// the scheduler would have no way to know it had.
#[derive(Debug, Clone, Default)]
pub struct ContactBatch {
    /// The contacts, in the order the backend reported them.
    pub contacts: Vec<Collision>,
}
