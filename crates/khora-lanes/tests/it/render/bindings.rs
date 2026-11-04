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

//! What a lit lane binds at group 3 besides its lights: the shadow atlases
//! and the image-based lighting. A frame without a shadow frame (the shadows
//! failed, or run headless) or without an IBL bake (not run yet) is still
//! drawn, through neutral bindings the lane owns — a 1×1 depth atlas of each
//! kind and a comparison sampler; black cubes, a flat BRDF lookup and a
//! filtering sampler — and the group-3 bind group is kept while what it binds
//! holds.

use khora_core::renderer::api::command::BindGroupId;
use khora_core::renderer::api::frame::MAX_FRAMES_IN_FLIGHT;
use khora_core::renderer::api::shadow::bindings::binding;

use super::fixtures::{Bound, GpuKind, RenderLedger, Renderer, Scene, LIT_LANES};

/// The IBL bindings after the base: irradiance, prefiltered, BRDF lookup,
/// sampler.
const IBL_VIEWS: [u32; 3] = [0, 1, 2];
const IBL_SAMPLER: u32 = 3;

/// The group-3 bind group the scene pass of `buffer` set last.
fn group_3(
    device: &RenderLedger,
    buffer: khora_core::renderer::api::command::CommandBufferId,
) -> BindGroupId {
    device
        .passes_into(buffer)
        .iter()
        .rev()
        .find_map(|pass| pass.bind_group(3))
        .expect("the scene pass binds group 3")
}

/// Whether `bound` is an object of `kind` the lane made itself.
fn lane_made(device: &RenderLedger, bound: Option<Bound>, kind: GpuKind) -> bool {
    match (bound, kind) {
        (Some(Bound::View(view)), GpuKind::TextureView) => device.lane_made_it(kind, view.0),
        (Some(Bound::Sampler(sampler)), GpuKind::Sampler) => device.lane_made_it(kind, sampler.0),
        _ => false,
    }
}

/// **Without a shadow frame or an IBL bake, a lit lane still draws.** The old
/// lanes skipped the whole lit render when either was missing; now the frame
/// is recorded and every mesh drawn.
#[test]
fn without_shadows_or_ibl_a_lit_lane_still_draws() {
    for which in LIT_LANES {
        let mut renderer = Renderer::new(which);
        let scene = Scene::new().with_meshes(2);
        assert!(scene.shadow.is_none());

        let recorded = renderer.record(&scene);
        let (scene_pass, _) = recorded.recorded();

        let draws: usize = renderer
            .device
            .passes_into(scene_pass.command_buffer)
            .iter()
            .map(|pass| pass.draws.len())
            .sum();
        assert_eq!(draws, 2, "{which:?}: both meshes drawn");
    }
}

/// **Without them, the lane binds neutral bindings of its own.** Group 3's
/// shadow atlases and comparison sampler, and its IBL views and sampler, are
/// objects the lane made — not the shadows family's, not a bake's.
#[test]
fn without_shadows_or_ibl_a_lit_lane_binds_its_own_neutral_bindings() {
    for which in LIT_LANES {
        let mut renderer = Renderer::new(which);

        let recorded = renderer.record(&Scene::new().with_meshes(1));
        let group = group_3(&renderer.device, recorded.recorded().0.command_buffer);

        let device = renderer.device.as_ref();
        for (at, kind, what) in [
            (binding::ATLAS_2D, GpuKind::TextureView, "the 2D atlas"),
            (binding::ATLAS_CUBE, GpuKind::TextureView, "the cube atlas"),
            (binding::SAMPLER, GpuKind::Sampler, "the comparison sampler"),
        ] {
            assert!(
                lane_made(device, device.bound_at(group, at), kind),
                "{which:?}: {what} at {at} is the lane's own: {:?}",
                device.bound_at(group, at)
            );
        }
        let base = which.ibl_base();
        for offset in IBL_VIEWS {
            assert!(
                lane_made(
                    device,
                    device.bound_at(group, base + offset),
                    GpuKind::TextureView
                ),
                "{which:?}: the IBL view at {} is the lane's own: {:?}",
                base + offset,
                device.bound_at(group, base + offset)
            );
        }
        assert!(
            lane_made(
                device,
                device.bound_at(group, base + IBL_SAMPLER),
                GpuKind::Sampler
            ),
            "{which:?}: the IBL sampler is the lane's own"
        );
    }
}

/// **A published shadow frame and a bake are bound instead.** With the
/// shadows family's frame on the deck and the IBL baked, group 3 binds the
/// shadows' atlases and sampler and the bake's irradiance, prefiltered and
/// BRDF views and sampler.
#[test]
fn a_published_shadow_frame_and_bake_are_bound_instead() {
    for which in LIT_LANES {
        let mut renderer = Renderer::new(which);
        let baker = renderer.bake();
        let ibl = baker.bindings().expect("the bake ran");
        let scene = Scene::with_baker(baker).with_meshes(1).with_shadow_frame();
        let shadows = scene.shadow_bindings();

        let recorded = renderer.record(&scene);
        let group = group_3(&renderer.device, recorded.recorded().0.command_buffer);

        let device = renderer.device.as_ref();
        assert_eq!(
            device.bound_at(group, binding::ATLAS_2D),
            Some(Bound::View(shadows.atlas_2d)),
            "{which:?}"
        );
        assert_eq!(
            device.bound_at(group, binding::ATLAS_CUBE),
            Some(Bound::View(shadows.atlas_cube)),
            "{which:?}"
        );
        assert_eq!(
            device.bound_at(group, binding::SAMPLER),
            Some(Bound::Sampler(shadows.sampler)),
            "{which:?}"
        );
        let base = which.ibl_base();
        for (offset, view) in
            IBL_VIEWS
                .into_iter()
                .zip([ibl.irradiance_cube, ibl.prefiltered_cube, ibl.brdf_lut])
        {
            assert_eq!(
                device.bound_at(group, base + offset),
                Some(Bound::View(view)),
                "{which:?}: the bake's view at {}",
                base + offset
            );
        }
        assert_eq!(
            device.bound_at(group, base + IBL_SAMPLER),
            Some(Bound::Sampler(ibl.sampler)),
            "{which:?}"
        );
    }
}

/// **The group-3 bind group is kept while what it binds holds.** Over six
/// frames with the same shadow frame and bake, the lane binds no more group-3
/// bind groups than it has frames in flight, and destroys none; the old lanes
/// made one and destroyed it every frame. A new shadow frame is bound at the
/// next frame.
#[test]
fn the_group_3_bind_group_is_kept_while_what_it_binds_holds() {
    for which in LIT_LANES {
        let mut renderer = Renderer::new(which);
        let baker = renderer.bake();
        let scene = Scene::with_baker(baker).with_meshes(1).with_shadow_frame();

        let mut groups = Vec::new();
        for _ in 0..6 {
            let recorded = renderer.record(&scene);
            groups.push(group_3(
                &renderer.device,
                recorded.recorded().0.command_buffer,
            ));
        }
        let mut distinct = groups.clone();
        distinct.sort_by_key(|group| group.0);
        distinct.dedup();
        assert!(
            distinct.len() <= MAX_FRAMES_IN_FLIGHT,
            "{which:?}: {} group-3 bind groups over six frames: {groups:?}",
            distinct.len()
        );
        for group in &distinct {
            assert!(
                !renderer.device.is_destroyed(GpuKind::BindGroup, group.0),
                "{which:?}: a bind group still in use is not destroyed"
            );
        }

        let moved = scene.with_shadow_frame();
        let recorded = renderer.record(&moved);
        let group = group_3(&renderer.device, recorded.recorded().0.command_buffer);
        assert_eq!(
            renderer.device.bound_at(group, binding::ATLAS_2D),
            Some(Bound::View(moved.shadow_bindings().atlas_2d)),
            "{which:?}: the new shadow frame is bound"
        );
    }
}
