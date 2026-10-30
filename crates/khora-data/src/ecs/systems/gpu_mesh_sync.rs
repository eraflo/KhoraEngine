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

//! CPU → GPU mesh sync — uploads any newly-spawned `HandleComponent<Mesh>`
//! to the device cache and tags the entity with `HandleComponent<GpuMesh>`.
//!
//! The [`MeshOnGpu`] residency: runs in `TickPhase::PreExtract`, after the
//! asset reload and the resolver, so it lands before `RenderFlow` projects
//! the world.

use crate::ecs::SystemContext;
use std::sync::Arc;

use khora_core::lane::OutputDeck;
use khora_core::renderer::GraphicsDevice;
use khora_core::Runtime;

use khora_core::residency::Residency;

use crate::ecs::World;
use crate::residency::MeshOnGpu;
use crate::ProjectionRegistry;

fn gpu_mesh_sync_system(
    world: &mut World,
    runtime: &Runtime,
    _deck: &mut OutputDeck,
    _ctx: SystemContext,
) {
    let Some(proj) = runtime.resources.get::<ProjectionRegistry>() else {
        return;
    };
    let Some(device) = runtime.backends.get::<Arc<dyn GraphicsDevice>>() else {
        return;
    };
    proj.sync_all(world, device.as_ref());
}

crate::register_residency!(name = MeshOnGpu::NAME, run = gpu_mesh_sync_system);
