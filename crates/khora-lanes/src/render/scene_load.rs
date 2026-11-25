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
use khora_core::renderer::api::gpu_scene::GpuMesh;
use khora_core::renderer::api::pipeline::enums::PrimitiveTopology;
use khora_core::sync::RwLockExt;
use khora_data::render::RenderWorld;
use khora_data::AssetStore;

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
    /// Measures the work of drawing `world` into `target`, each mesh counted
    /// by the GPU copy `assets` holds under its uuid.
    ///
    /// A mesh with no resident GPU copy is not drawn, so it counts nothing.
    pub fn of(world: &RenderWorld, assets: &AssetStore, target: &SceneTarget) -> Self {
        let gpu_meshes = assets.store::<GpuMesh>();
        let gpu_meshes = gpu_meshes.read_recovered();
        let mut draws = 0u32;
        let mut triangles = 0u64;
        for gpu_mesh in world
            .meshes
            .iter()
            .filter_map(|mesh| gpu_meshes.get(&mesh.mesh))
        {
            draws += 1;
            let indices = u64::from(gpu_mesh.index_count);
            triangles += match gpu_mesh.primitive_topology {
                PrimitiveTopology::TriangleList => indices / 3,
                PrimitiveTopology::TriangleStrip => indices.saturating_sub(2),
                PrimitiveTopology::LineList
                | PrimitiveTopology::LineStrip
                | PrimitiveTopology::PointList => 0,
            };
        }
        Self {
            draws,
            triangles,
            suns: world.directional_light_count(),
            lamps: world.point_light_count(),
            spots: world.spot_light_count(),
            pixels: f64::from(target.size.width) * f64::from(target.size.height),
        }
    }

    /// Counts every light of the frame.
    pub fn lights(&self) -> usize {
        self.suns + self.lamps + self.spots
    }

    /// Returns what recording the frame costs the host.
    pub(crate) fn host_time(&self) -> Duration {
        RECORD_CPU + DRAW_CPU * self.draws
    }

    /// Returns what the device spends on the frame's geometry: the pass, the draws
    /// and their triangles.
    pub(crate) fn geometry_time(&self) -> Duration {
        PASS_GPU
            + DRAW_GPU * self.draws
            + Duration::from_secs_f64(self.triangles as f64 * TRIANGLE_GPU_SECS)
    }

    /// Returns what the device spends filling the target, each pixel shaded at
    /// `shading` times the cost of an unlit one.
    pub(crate) fn fill_time(&self, shading: f64) -> Duration {
        Duration::from_secs_f64(self.pixels * PIXEL_GPU_SECS * shading.max(0.0))
    }
}
