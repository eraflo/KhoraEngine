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

//! IBL environment bake, wired as a data-driven system.
//!
//! Runs in [`TickPhase::PreExtract`] so the baked environment is ready before
//! any lit lane renders. Each tick it reads the scene's environment source —
//! the selected texture and the handle the store holds for it — and the
//! [`IblBaker`] bakes when that source differs from the one it baked from: the
//! first frame, a texture repainted on disk (hot reload replaces the store's
//! handle), another texture selected, or a late-loading environment replacing
//! the procedural fallback. Otherwise a tick is one uuid and one pointer
//! compare.

use std::sync::Arc;

use khora_core::lane::OutputDeck;
use khora_core::math::Vec3;
use khora_core::renderer::api::resource::CpuTexture;
use khora_core::renderer::light::LightType;
use khora_core::renderer::traits::PipelineSystem;
use khora_core::renderer::GraphicsDevice;
use khora_core::Runtime;

use crate::ecs::{DataSystemRegistration, TickPhase, World};
#[cfg(test)]
use crate::ecs::{GlobalTransform, Light};
use crate::{AssetStore, EnvironmentMap, IblBaker, IblSource};

/// Returns the world-space unit direction **toward** the scene's first
/// directional light that shines ([`shining_lights`](crate::render::shining_lights)),
/// or `Vec3::ZERO` when there is none (the bake then falls back to its own
/// default sun).
///
/// The light's travel direction is the one the render extraction publishes;
/// the sun sits opposite that. A first sun whose values make no light never
/// hides a second one, nor poisons the sky.
fn sun_direction(world: &World) -> Vec3 {
    crate::render::shining_lights(world)
        .into_iter()
        .find(|shining| matches!(shining.light.light_type, LightType::Directional(_)))
        .map_or(Vec3::ZERO, |sun| -sun.direction)
}

/// What one tick of the bake does with the scene's current environment source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decision {
    /// The current bake was made from this source: nothing to do.
    Skip,
    /// The selected environment is still loading: keep the current bake and
    /// look again next tick.
    Wait,
    /// Bake from the source (its texture, or the procedural sky when it has
    /// none).
    Bake,
}

/// Decides what this tick does with `source` — keep the bake, wait for the
/// selected environment to load, or bake. Spends one tick of the baker's
/// bounded environment wait when it waits.
fn bake_decision(baker: &IblBaker, source: &IblSource) -> Decision {
    if baker.is_baked_from(source) {
        return Decision::Skip;
    }
    let selected_but_loading = source.environment.is_some() && source.texture.is_none();
    if selected_but_loading && baker.wait_for_environment() {
        // Retry next tick rather than baking a procedural sky the scene did
        // not ask for. Bounded: before the first bake the lit lanes render
        // nothing, so an asset that never arrives must not stall them forever.
        return Decision::Wait;
    }
    Decision::Bake
}

fn ibl_bake_system(world: &mut World, runtime: &Runtime, _deck: &mut OutputDeck) {
    let Some(baker) = runtime.resources.get::<IblBaker>() else {
        return;
    };
    // The scene's environment: the selected texture and the handle the store
    // holds for it now. The handle is shared, so the store's lock is not held
    // across the bake.
    let environment = runtime
        .resources
        .get::<EnvironmentMap>()
        .and_then(|env| env.texture);
    let texture = environment.and_then(|uuid| {
        let store = runtime.resources.get::<AssetStore>()?.store::<CpuTexture>();
        let guard = store.read().unwrap_or_else(|e| e.into_inner());
        guard.get(&uuid).cloned()
    });
    let source = IblSource {
        environment,
        texture,
    };
    if bake_decision(baker, &source) != Decision::Bake {
        return;
    }
    let Some(device) = runtime.backends.get::<Arc<dyn GraphicsDevice>>() else {
        return;
    };
    let Some(pipeline_system) = runtime.resources.get::<Arc<dyn PipelineSystem>>() else {
        return;
    };
    if let (Some(uuid), None) = (source.environment, &source.texture) {
        log::warn!(
            "ibl_bake: environment texture {uuid:?} did not load in time; baking the \
             procedural sky instead"
        );
    }
    baker.ensure_baked(
        device.as_ref(),
        pipeline_system.as_ref(),
        sun_direction(world),
        &source,
    );
}

inventory::submit! {
    DataSystemRegistration {
        name: "ibl_bake",
        phase: TickPhase::PreExtract,
        run: ibl_bake_system,
        order_hint: -10,
        runs_after: &[],
        observes: &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::asset::{AssetHandle, AssetUUID};
    use khora_core::renderer::api::resource::{
        SampleCount, TextureDimension, TextureFormat, TextureUsage,
    };

    /// A 1×1 RGBA texture: enough to stand for a loaded environment.
    fn texture() -> AssetHandle<CpuTexture> {
        AssetHandle::new(CpuTexture {
            pixels: vec![255; 4],
            size: khora_core::math::Extent3D {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            format: TextureFormat::Rgba8Unorm,
            mip_level_count: 1,
            sample_count: SampleCount::X1,
            dimension: TextureDimension::D2,
            usage: TextureUsage::COPY_DST | TextureUsage::TEXTURE_BINDING,
        })
    }

    fn sky() -> AssetUUID {
        AssetUUID::new_v5("test/sky.hdr")
    }

    fn loaded(texture: &AssetHandle<CpuTexture>) -> IblSource {
        IblSource {
            environment: Some(sky()),
            texture: Some(texture.clone()),
        }
    }

    fn selected_not_loaded() -> IblSource {
        IblSource {
            environment: Some(sky()),
            texture: None,
        }
    }

    /// Spends the baker's whole environment wait, as ticks of a selected
    /// environment that never loads would.
    fn spend_the_wait(baker: &IblBaker) {
        for _ in 0..100_000 {
            if !baker.wait_for_environment() {
                return;
            }
        }
        panic!("the environment wait is bounded");
    }

    #[test]
    fn a_bake_from_the_current_source_is_kept() {
        let a = texture();
        let baker = IblBaker::baked_for_test(loaded(&a));
        assert_eq!(bake_decision(&baker, &loaded(&a)), Decision::Skip);
    }

    #[test]
    fn the_procedural_sky_baked_without_an_environment_is_kept() {
        let baker = IblBaker::baked_for_test(IblSource::procedural());
        assert_eq!(
            bake_decision(&baker, &IblSource::procedural()),
            Decision::Skip
        );
    }

    #[test]
    fn a_replaced_environment_texture_is_baked_again() {
        let a = texture();
        let b = texture();
        let baker = IblBaker::baked_for_test(loaded(&a));
        assert_eq!(bake_decision(&baker, &loaded(&b)), Decision::Bake);
    }

    #[test]
    fn an_environment_loading_late_replaces_the_procedural_fallback() {
        let baker = IblBaker::baked_for_test(selected_not_loaded());
        assert_eq!(bake_decision(&baker, &loaded(&texture())), Decision::Bake);
    }

    #[test]
    fn the_procedural_fallback_is_kept_while_the_environment_stays_unloaded() {
        let baker = IblBaker::baked_for_test(selected_not_loaded());
        assert_eq!(
            bake_decision(&baker, &selected_not_loaded()),
            Decision::Skip
        );
    }

    #[test]
    fn a_selected_environment_not_loaded_is_waited_for() {
        let baker = IblBaker::new();
        assert_eq!(
            bake_decision(&baker, &selected_not_loaded()),
            Decision::Wait
        );
    }

    #[test]
    fn a_selected_environment_never_loaded_bakes_the_sky_once_the_wait_is_spent() {
        let baker = IblBaker::new();
        spend_the_wait(&baker);
        assert_eq!(
            bake_decision(&baker, &selected_not_loaded()),
            Decision::Bake
        );
    }

    #[test]
    fn an_unbaked_baker_with_no_environment_bakes_the_sky() {
        let baker = IblBaker::new();
        assert_eq!(
            bake_decision(&baker, &IblSource::procedural()),
            Decision::Bake
        );
    }

    #[test]
    fn an_environment_selected_after_the_sky_is_waited_for_while_it_loads() {
        let baker = IblBaker::baked_for_test(IblSource::procedural());
        assert_eq!(
            bake_decision(&baker, &selected_not_loaded()),
            Decision::Wait
        );
    }

    #[test]
    fn a_cleared_environment_bakes_the_sky_again() {
        let baker = IblBaker::baked_for_test(loaded(&texture()));
        assert_eq!(
            bake_decision(&baker, &IblSource::procedural()),
            Decision::Bake
        );
    }

    /// The sun the environment is baked with sits exactly opposite the
    /// direction the render extraction lights the scene with — under a
    /// stretched parent too, where the light's world direction leans with
    /// the sheared basis rather than turning with its rotation alone.
    #[test]
    fn the_baked_sun_sits_opposite_the_rendered_light_under_a_stretched_parent() {
        use crate::flow::{Flow, RenderFlow, Selection};
        use khora_core::math::affine_transform::AffineTransform;
        use khora_core::math::{Mat4, Quaternion};
        use khora_core::renderer::light::DirectionalLight;

        let pose = AffineTransform(
            Mat4::from_scale(Vec3::new(1.0, 3.0, 1.0))
                * Mat4::from_quat(Quaternion::from_axis_angle(
                    Vec3::new(1.0, 0.5, -0.3).normalize(),
                    0.9,
                )),
        );
        let mut world = World::new();
        world.spawn((
            GlobalTransform(pose),
            Light::new(LightType::Directional(DirectionalLight {
                direction: Vec3::new(0.3, -1.0, 0.2).normalize(),
                ..DirectionalLight::default()
            })),
        ));

        let rendered = RenderFlow.project(&world, &Selection::new(), &Runtime::default());
        assert_eq!(rendered.lights.len(), 1, "the one light is extracted");
        let sun = sun_direction(&world);
        let error = (sun + rendered.lights[0].direction).length();
        assert!(
            error < 1e-6,
            "the sun {sun:?} is not opposite the rendered direction {:?} (off by {error})",
            rendered.lights[0].direction
        );
    }

    /// A first sun pointing nowhere (an infinite direction) does not hide the
    /// valid sun after it: the sky is baked toward the second — its
    /// direction negated, unit.
    #[test]
    fn the_sky_takes_the_first_sun_that_shines() {
        use khora_core::renderer::light::DirectionalLight;

        let second = Vec3::new(0.3, -1.0, 0.2);
        let mut world = World::new();
        world.spawn((
            GlobalTransform::identity(),
            Light::new(LightType::Directional(DirectionalLight {
                direction: Vec3::new(f32::INFINITY, -1.0, 0.0),
                ..DirectionalLight::default()
            })),
        ));
        world.spawn((
            GlobalTransform::identity(),
            Light::new(LightType::Directional(DirectionalLight {
                direction: second,
                ..DirectionalLight::default()
            })),
        ));

        let sun = sun_direction(&world);

        let expected = -second.normalize();
        assert!(
            (sun - expected).length() < 1e-6,
            "the sky is baked toward {sun:?}, not toward the second sun {expected:?}"
        );
    }
}
