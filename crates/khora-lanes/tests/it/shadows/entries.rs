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

//! The `ShadowFrame` the lane commits for the lit lanes: one entry per
//! shadowed light, keyed by its index in `RenderWorld.lights`, and the atlases
//! to bind — read by every lit lane through the deck, whichever level ran.

use khora_core::lane::erased::RunOutcome;
use khora_core::lane::guard::{Reason, SourceKind};
use khora_core::lane::OutputDeck;
use khora_core::math::Vec3;
use khora_core::renderer::api::resource::TextureViewDimension;
use khora_core::renderer::api::shadow::{ShadowEntry, ShadowFrame};

use super::fixtures::{faces, view_proj, Scene, Shadows, HIGH, LOW};

/// **A shadowed sun gets a layer of the 2D atlas**, its entry carrying the
/// view-projection the `ShadowView` gave it.
#[test]
fn a_shadowed_sun_gets_a_layer_of_the_2d_atlas() {
    let mut shadows = Shadows::new();
    let sun = view_proj(3.0);

    let recorded = shadows.record(HIGH, &Scene::new().with_sun(sun));
    let (frame, _) = recorded.recorded();

    match frame.entry(0) {
        Some(ShadowEntry::Atlas2D {
            view_proj,
            atlas_index,
        }) => {
            assert_eq!(*view_proj, sun, "the ShadowView's matrix");
            assert_eq!(*atlas_index, 0, "the first layer");
        }
        other => panic!("expected the sun in the 2D atlas, got {other:?}"),
    }
    assert_eq!(frame.entries.len(), 1);
}

/// **A shadowed point light gets a cube of the cube atlas**, its entry
/// carrying its six face matrices, its position, and its range as the far
/// plane the shader compares depths against.
#[test]
fn a_shadowed_point_light_gets_a_cube_of_the_cube_atlas() {
    let mut shadows = Shadows::new();
    let at = Vec3::new(1.0, 2.0, 3.0);
    let six = faces(2.0);

    let recorded = shadows.record(LOW, &Scene::new().with_lamp(at, 12.5, six));
    let (frame, _) = recorded.recorded();

    match frame.entry(0) {
        Some(ShadowEntry::Cube {
            face_view_projs,
            cube_array_index,
            light_pos,
            far_plane,
        }) => {
            assert_eq!(**face_view_projs, six, "the ShadowView's six matrices");
            assert_eq!(*cube_array_index, 0, "the first cube");
            assert_eq!(*light_pos, at);
            assert_eq!(*far_plane, 12.5, "the light's range");
        }
        other => panic!("expected the lamp in the cube atlas, got {other:?}"),
    }
}

/// A light the `ShadowView` gives no matrix casts no shadow; the lights after
/// it keep the index they have in `RenderWorld.lights`.
#[test]
fn entries_are_keyed_by_the_lights_index_in_the_render_world() {
    let mut shadows = Shadows::new();
    let sun = view_proj(4.0);

    let recorded = shadows.record(HIGH, &Scene::new().with_unshadowed_sun().with_sun(sun));
    let (frame, _) = recorded.recorded();

    assert!(frame.entry(0).is_none(), "the unshadowed sun has no entry");
    assert!(
        matches!(frame.entry(1), Some(ShadowEntry::Atlas2D { view_proj, .. }) if *view_proj == sun),
        "the shadowed sun keeps its index: {:?}",
        frame.entry(1)
    );
}

/// **The frame names the atlases the lane drew into**: the 2D-array view,
/// the cube-array view and the comparison sampler it built — so the lit lanes
/// sample exactly what this frame's passes filled.
#[test]
fn the_frame_names_the_atlases_the_lane_drew_into() {
    let mut shadows = Shadows::new();

    let recorded = shadows.record(HIGH, &Scene::new().with_sun(view_proj(1.0)));
    let (frame, _) = recorded.recorded();

    let bindings = frame.bindings().expect("a prepared lane names its atlases");
    assert_eq!(
        shadows.device.views_of(TextureViewDimension::D2Array),
        vec![bindings.atlas_2d],
        "the one live 2D-array view"
    );
    assert_eq!(
        shadows.device.views_of(TextureViewDimension::CubeArray),
        vec![bindings.atlas_cube],
        "the one live cube-array view"
    );
    assert_eq!(
        shadows.device.samplers(),
        vec![bindings.sampler],
        "the one live sampler"
    );
}

/// **A frame's shadows replace what the slot held.** The deck output opens
/// on what its slot holds this frame; the shadows lane writes the whole frame,
/// so an entry the slot held for a light that casts no shadow now is gone.
#[test]
fn the_shadow_frame_replaces_what_its_slot_held() {
    let mut shadows = Shadows::new();
    let mut seed = OutputDeck::new();
    seed.slot::<ShadowFrame>().entries.insert(
        5,
        ShadowEntry::Atlas2D {
            view_proj: view_proj(9.0),
            atlas_index: 3,
        },
    );

    let recorded = shadows.record_seeded(HIGH, &Scene::new().with_sun(view_proj(1.0)), &seed);
    let (frame, _) = recorded.recorded();

    assert!(frame.entry(5).is_none(), "the stale entry is gone");
    assert_eq!(frame.entries.len(), 1, "only this frame's sun");
}

/// Without a `ShadowView` on the bus the lane is refused by name, and commits
/// nothing: the lit lanes draw without shadows this frame.
#[test]
fn a_frame_without_a_shadow_view_is_refused() {
    let mut shadows = Shadows::new();

    let recorded = shadows.record_without_shadow_view(HIGH, &Scene::new().with_sun(view_proj(1.0)));

    match recorded.run.outcome {
        RunOutcome::Refused(refusal) => {
            assert_eq!(refusal.source, SourceKind::Bus);
            assert_eq!(refusal.reason, Reason::Missing);
        }
        ref other => panic!("expected the missing ShadowView to be refused, got {other:?}"),
    }
    assert!(recorded.committed_nothing());
    assert!(shadows.device.opened().is_empty(), "no pass was opened");
}
