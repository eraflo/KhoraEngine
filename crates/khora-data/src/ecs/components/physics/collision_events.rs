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

use khora_core::physics::CollisionEvent;
use khora_macros::Component;
use serde::{Deserialize, Serialize};

/// A component that stores collision events for the current frame.
/// Typically attached to a singleton entity or used as a resource.
#[derive(Debug, Clone, Default, Component, Serialize, Deserialize)]
pub struct CollisionEvents {
    /// List of events that occurred in the last physics step.
    pub events: Vec<CollisionEvent>,
}
