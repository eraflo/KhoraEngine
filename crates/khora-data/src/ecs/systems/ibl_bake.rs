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

//! One-time IBL environment bake, wired as a data-driven system.
//!
//! Runs in [`TickPhase::PreExtract`] so the baked environment is ready before
//! any lit lane renders. The [`IblBaker`] bakes only on its first call and is
//! a cheap no-op every frame after — no dirty tracking needed here.

use std::sync::Arc;

use khora_core::lane::OutputDeck;
use khora_core::math::Vec3;
use khora_core::renderer::light::LightType;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;
use khora_core::Runtime;

use crate::ecs::{DataSystemRegistration, GlobalTransform, Light, TickPhase, World};
use crate::IblBaker;

/// Returns the world-space direction **toward** the scene's first enabled
/// directional light, or `Vec3::ZERO` when there is none (the bake then falls
/// back to its own default sun).
///
/// The light's travel direction is derived exactly as the render extraction
/// does it (`rotation * direction`); the sun sits opposite that.
fn sun_direction(world: &World) -> Vec3 {
    for (light, transform) in world.query::<(&Light, &GlobalTransform)>() {
        if !light.enabled {
            continue;
        }
        if let LightType::Directional(dir_light) = &light.light_type {
            return -(transform.0.rotation() * dir_light.direction);
        }
    }
    Vec3::ZERO
}

fn ibl_bake_system(world: &mut World, runtime: &Runtime, _deck: &mut OutputDeck) {
    let Some(baker) = runtime.resources.get::<IblBaker>() else {
        return;
    };
    let Some(device) = runtime.backends.get::<Arc<dyn GraphicsDevice>>() else {
        return;
    };
    let Some(pipeline_system) = runtime.resources.get::<Arc<dyn PipelineSystem>>() else {
        return;
    };
    baker.ensure_baked(
        device.as_ref(),
        pipeline_system.as_ref(),
        sun_direction(world),
    );
}

inventory::submit! {
    DataSystemRegistration {
        name: "ibl_bake",
        phase: TickPhase::PreExtract,
        run: ibl_bake_system,
        order_hint: -10,
        runs_after: &[],
    }
}
