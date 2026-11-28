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

//! Residencies: meshes, materials and UI images made usable by the GPU across frames.
//!
//! Each is a [`Residency`](khora_core::residency::Residency) registered with
//! [`register_residency!`](crate::register_residency).

mod image_on_gpu;
mod material_on_gpu;
mod mesh_on_gpu;

pub use image_on_gpu::{AtlasTarget, ImageOnGpu};
pub use mesh_on_gpu::MeshOnGpu;

/// Registers a residency's `DataSystem`: at `PreExtract`, after the asset
/// reload and the resolver, so it sees this frame's copies of the assets.
///
/// `observes` names the components whose changes the residency reads with
/// `Changed<T>` since its last run.
///
/// ```ignore
/// register_residency!(name = ImageOnGpu::NAME, run = image_on_gpu_system);
/// register_residency!(
///     name = "gpu_material_sync",
///     run = sync,
///     after = ["gpu_mesh_sync"],
///     observes = [AssetRef<Box<dyn Material>>],
/// );
/// ```
#[macro_export]
macro_rules! register_residency {
    (
        name = $name:expr,
        run = $run:path
        $(, after = [$($after:expr),* $(,)?])?
        $(, observes = [$($observed:ty),* $(,)?])?
        $(,)?
    ) => {
        inventory::submit! {
            $crate::ecs::DataSystemRegistration {
                name: $name,
                phase: $crate::ecs::TickPhase::PreExtract,
                run: $run,
                order_hint: 0,
                runs_after: &["asset_reload", "asset_resolver" $($(, $after)*)?],
                observes: &[$($($crate::ecs::ComponentKey::of::<$observed>),*)?],
                role: $crate::ecs::Role::Upkeep,
            }
        }
    };
}

#[cfg(test)]
mod tests;
