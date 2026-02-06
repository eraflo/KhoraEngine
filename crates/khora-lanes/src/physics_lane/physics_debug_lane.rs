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

use crate::physics_lane::PhysicsLane;
use khora_core::physics::PhysicsProvider;
use khora_data::ecs::{PhysicsDebugData, World};

/// A lane dedicated to extracting debug information from the physics engine.
#[derive(Debug, Default)]
pub struct PhysicsDebugLane;

impl PhysicsDebugLane {
    /// Creates a new `PhysicsDebugLane`.
    pub fn new() -> Self {
        Self
    }
}

impl PhysicsLane for PhysicsDebugLane {
    fn strategy_name(&self) -> &'static str {
        "PhysicsDebug"
    }

    fn step(&self, world: &mut World, provider: &mut dyn PhysicsProvider, _dt: f32) {
        // We look for a PhysicsDebugData component in the world.
        // It's expected to be a singleton or attached to a specific debug entity.
        let query = world.query_mut::<&mut PhysicsDebugData>();
        for debug_data in query {
            if debug_data.enabled {
                let (vertices, indices) = provider.get_debug_render_data();
                debug_data.vertices = vertices;
                debug_data.indices = indices;
            } else {
                debug_data.vertices.clear();
                debug_data.indices.clear();
            }
        }
    }
}
