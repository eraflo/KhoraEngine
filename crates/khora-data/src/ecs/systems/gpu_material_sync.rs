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

//! CPU → GPU material sync — uploads each entity's material (uniforms +
//! PBR textures + group-2 bind group) to the device cache and tags the
//! entity with `HandleComponent<GpuMaterial>`.
//!
//! Runs in [`TickPhase::PreExtract`], after `gpu_mesh_sync` (so every
//! rendered entity already has a `HandleComponent<GpuMesh>`) and before
//! `RenderFlow` projects the world. Decoded CPU textures are read from the
//! shared `Assets<CpuTexture>` sub-store of the [`AssetStore`](crate::gpu::AssetStore),
//! populated by the SDK layer that owns the `AssetService` — this system
//! performs no asset loading itself, keeping the data layer free of any
//! `khora-io` dependency.

use std::sync::Arc;

use khora_core::lane::OutputDeck;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;
use khora_core::Runtime;

use crate::ecs::{DataSystemRegistration, TickPhase, World};
use crate::ProjectionRegistry;

fn gpu_material_sync_system(world: &mut World, runtime: &Runtime, _deck: &mut OutputDeck) {
    let Some(proj) = runtime.resources.get::<ProjectionRegistry>() else {
        return;
    };
    let Some(device) = runtime.backends.get::<Arc<dyn GraphicsDevice>>() else {
        return;
    };
    // The per-variant group-2 material layout is owned by the PipelineSystem
    // backend, so each GpuMaterial's bind group shares the exact layout the
    // lit pipeline binds for that material's variant.
    let Some(pipeline_system) = runtime.resources.get::<Arc<dyn PipelineSystem>>() else {
        return;
    };
    proj.sync_materials(world, device.as_ref(), pipeline_system.as_ref());
}

inventory::submit! {
    DataSystemRegistration {
        name: "gpu_material_sync",
        phase: TickPhase::PreExtract,
        run: gpu_material_sync_system,
        order_hint: 1,
        runs_after: &["gpu_mesh_sync"],
    }
}
