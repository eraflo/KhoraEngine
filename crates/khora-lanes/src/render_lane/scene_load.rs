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

//! What a frame asks of a render lane, counted for its quote: the draws, the
//! triangles, the lights and the pixels.

use std::time::Duration;

use khora_core::lane::io::SceneTarget;
use khora_core::renderer::api::pipeline::enums::PrimitiveTopology;
use khora_data::render::RenderWorld;

/// What recording a frame costs the host before any draw.
const RECORD_CPU: Duration = Duration::from_micros(20);
/// What one draw costs the host: its uniforms uploaded, its commands recorded.
const DRAW_CPU: Duration = Duration::from_micros(2);
/// What a scene pass costs the device before any draw.
const PASS_GPU: Duration = Duration::from_micros(10);
/// What one draw costs the device: its state set, its vertices fetched.
const DRAW_GPU: Duration = Duration::from_micros(1);
/// What one triangle costs the device, in seconds.
const TRIANGLE_GPU_SECS: f64 = 2e-9;
/// What filling one pixel costs the device, in seconds, before lighting.
const PIXEL_GPU_SECS: f64 = 5e-11;

/// The work one frame of the scene holds.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct SceneLoad {
    /// The meshes drawn.
    pub draws: u32,
    /// Their triangles.
    pub triangles: u64,
    /// The directional lights.
    pub suns: usize,
    /// The point lights.
    pub lamps: usize,
    /// The spot lights.
    pub spots: usize,
    /// The target's pixels.
    pub pixels: f64,
}

impl SceneLoad {
    /// The work of drawing `world` into `target`.
    pub fn of(world: &RenderWorld, target: &SceneTarget) -> Self {
        let triangles = world
            .meshes
            .iter()
            .map(|mesh| {
                let indices = u64::from(mesh.gpu_mesh.index_count);
                match mesh.gpu_mesh.primitive_topology {
                    PrimitiveTopology::TriangleList => indices / 3,
                    PrimitiveTopology::TriangleStrip => indices.saturating_sub(2),
                    PrimitiveTopology::LineList
                    | PrimitiveTopology::LineStrip
                    | PrimitiveTopology::PointList => 0,
                }
            })
            .sum();
        Self {
            draws: world.meshes.len() as u32,
            triangles,
            suns: world.directional_light_count(),
            lamps: world.point_light_count(),
            spots: world.spot_light_count(),
            pixels: f64::from(target.size.width) * f64::from(target.size.height),
        }
    }

    /// Every light of the frame.
    pub fn lights(&self) -> usize {
        self.suns + self.lamps + self.spots
    }

    /// What recording the frame costs the host.
    pub fn host_time(&self) -> Duration {
        RECORD_CPU + DRAW_CPU * self.draws
    }

    /// What the device spends on the frame's geometry: the pass, the draws
    /// and their triangles.
    pub fn geometry_time(&self) -> Duration {
        PASS_GPU
            + DRAW_GPU * self.draws
            + Duration::from_secs_f64(self.triangles as f64 * TRIANGLE_GPU_SECS)
    }

    /// What the device spends filling the target, each pixel shaded at
    /// `shading` times the cost of an unlit one.
    pub fn fill_time(&self, shading: f64) -> Duration {
        Duration::from_secs_f64(self.pixels * PIXEL_GPU_SECS * shading.max(0.0))
    }
}
