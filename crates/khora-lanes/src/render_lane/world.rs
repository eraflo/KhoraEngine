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

//! Defines the intermediate `RenderWorld` and its associated data structures.
//!
//! The `RenderWorld` is a temporary, frame-by-frame representation of the scene,
//! optimized for consumption by the rendering pipelines (`RenderLane`s). It is
//! populated by an "extraction" phase that reads data from the main ECS `World`.

use khora_core::{asset::AssetUUID, math::affine_transform::AffineTransform};

/// A flat, GPU-friendly representation of a single mesh to be rendered.
///
/// This struct contains all the necessary information, copied from various ECS
/// components, required to issue a draw call for a mesh.
pub struct ExtractedMesh {
    /// The world-space transformation matrix of the mesh, derived from `GlobalTransform`.
    pub transform: AffineTransform,
    /// The unique identifier of the GpuMesh asset to be rendered.
    pub gpu_mesh_uuid: AssetUUID,
    /// The unique identifier of the material to be used for rendering.
    /// If `None`, a default material should be used.
    pub material_uuid: Option<AssetUUID>,
}

/// A collection of all data extracted from the main `World` needed for rendering a single frame.
///
/// This acts as the primary input to the entire rendering system. By decoupling
/// from the main ECS, the render thread can work on this data without contention
/// while the simulation thread advances the next frame.
#[derive(Default)]
pub struct RenderWorld {
    /// A list of all meshes to be rendered in the current frame.
    pub meshes: Vec<ExtractedMesh>,
    // Future work: other lists for lights, cameras, etc.
    // pub lights: Vec<ExtractedLight>,
}

impl RenderWorld {
    /// Creates a new, empty `RenderWorld`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Clears all the data in the `RenderWorld`, preparing it for the next frame's extraction.
    pub fn clear(&mut self) {
        self.meshes.clear();
    }
}
