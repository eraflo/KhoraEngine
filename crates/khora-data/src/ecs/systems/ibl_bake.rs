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
use khora_core::renderer::api::resource::CpuTexture;
use khora_core::renderer::light::LightType;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;
use khora_core::Runtime;

use crate::ecs::{DataSystemRegistration, GlobalTransform, Light, TickPhase, World};
use crate::{AssetStore, EnvironmentMap, IblBaker};

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
    if baker.is_baked() {
        return;
    }
    let Some(device) = runtime.backends.get::<Arc<dyn GraphicsDevice>>() else {
        return;
    };
    let Some(pipeline_system) = runtime.resources.get::<Arc<dyn PipelineSystem>>() else {
        return;
    };
    let sun = sun_direction(world);

    // An authored equirectangular environment, when the scene selected one and
    // the asset is loaded. The read guard is held across the bake so the
    // texture cannot be evicted mid-projection; absent ⇒ procedural sky.
    let env_store = runtime
        .resources
        .get::<EnvironmentMap>()
        .and_then(|env| env.texture)
        .and_then(|uuid| {
            let store = runtime.resources.get::<AssetStore>()?.store::<CpuTexture>();
            Some((uuid, store))
        });
    match env_store {
        Some((uuid, store)) => {
            let guard = match store.read() {
                Ok(guard) => guard,
                Err(_) => {
                    log::error!("ibl_bake: CpuTexture store poisoned; baking procedural sky");
                    baker.ensure_baked(device.as_ref(), pipeline_system.as_ref(), sun, None);
                    return;
                }
            };
            if guard.get(&uuid).is_none() {
                // Still loading — retry next tick rather than baking a
                // procedural sky the scene did not ask for. Bounded: the lit
                // lanes do not render until the bake publishes its bindings,
                // so an asset that never arrives must not stall them forever.
                if baker.wait_for_environment() {
                    return;
                }
                log::warn!(
                    "ibl_bake: environment texture {uuid:?} did not load in time; \
                     baking the procedural sky instead"
                );
                drop(guard);
                baker.ensure_baked(device.as_ref(), pipeline_system.as_ref(), sun, None);
                return;
            }
            baker.ensure_baked(
                device.as_ref(),
                pipeline_system.as_ref(),
                sun,
                guard.get(&uuid).map(|handle| &**handle),
            );
        }
        None => baker.ensure_baked(device.as_ref(), pipeline_system.as_ref(), sun, None),
    }
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
