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

//! The atlas bind group while the image atlas has no texture yet: set 2
//! binds a one-texel texture the lane made, once, until the atlas has one;
//! the lane gives it back at shutdown; and a frame that fails on the way —
//! whether building it or growing the storage — leaves the next frame
//! drawing its own glyphs.

use std::sync::Arc;

use khora_core::renderer::api::resource::TextureViewId;
use khora_data::ui::UiImageAtlas;

use super::fixtures::{
    assert_everything_released, atlas_view, Bound, GpuKind, Ledger, Scene, Ui, OLD_CAPACITY,
    RESIDENT_GLYPHS,
};

/// An image atlas whose GPU texture does not exist yet, as the engine
/// inserts it before any UI image is wanted.
fn empty_atlas() -> Arc<UiImageAtlas> {
    Arc::new(UiImageAtlas::new())
}

/// The view the `n`-th quad pass binds at group 2.
fn bound_view(ui: &Ui, n: usize) -> TextureViewId {
    let group = ui.quad_pass(n).bind_group(2).expect("the atlas at group 2");
    match ui.device.bound_at(group, 0) {
        Some(Bound::View(view)) => view,
        other => panic!("a view at binding 0 of group 2, not {other:?}"),
    }
}

/// Asserts the last frame drew the resident text's own glyphs, each once:
/// nothing a failed frame computed was left for it.
fn assert_last_frame_draws_its_own_glyphs(ui: &Ui) {
    let drawn = ui.last_glyphs();
    assert_eq!(
        drawn.len(),
        RESIDENT_GLYPHS,
        "the frame draws its own glyphs: {drawn:?}"
    );
}

/// **An atlas without a texture binds a one-texel texture the lane made**,
/// made once: three frames over the empty atlas bind one view of the lane's
/// own, through one bind group.
#[test]
fn an_atlas_without_a_texture_binds_a_one_texel_texture_the_lane_made() {
    let mut ui = Ui::new();
    let scene = Scene::new().with_atlas(empty_atlas());

    for _ in 0..3 {
        ui.record(&scene).recorded();
    }

    let view = bound_view(&ui, 0);
    assert!(
        ui.device.find(GpuKind::TextureView, view.0).is_some(),
        "group 2 binds a view the lane made: {view:?}"
    );
    assert_eq!(
        ui.device.made_of(GpuKind::Texture).len(),
        1,
        "one fallback texture for three frames: {:?}",
        ui.device.made()
    );
    assert_eq!(ui.device.made_of(GpuKind::TextureView).len(), 1);
    let group = ui.quad_pass(0).bind_group(2);
    for frame in 1..3 {
        assert_eq!(ui.quad_pass(frame).bind_group(2), group, "frame {frame}");
    }
}

/// **The atlas's texture, once made, replaces the fallback binding**, and
/// the fallback is kept for an atlas that has none again: the frame after
/// the atlas gets its texture binds the atlas's view through a new bind
/// group, the one over the fallback given back once; a later frame over an
/// atlas without a texture binds the same fallback view again, making no
/// texture. Shutdown gives back every one of them, the fallback included,
/// and nothing the atlas owns.
#[test]
fn the_atlas_texture_once_made_replaces_the_fallback_binding() {
    let mut ui = Ui::new();
    let atlas = empty_atlas();

    ui.record(&Scene::new().with_atlas(Arc::clone(&atlas)))
        .recorded();
    let fallback_view = bound_view(&ui, 0);
    let fallback_group = ui.quad_pass(0).bind_group(2).expect("the fallback");

    assert!(
        atlas.ensure_atlas(&Ledger::counting_from(8_500_000)),
        "the residency makes the atlas on a device of its own"
    );
    ui.record(&Scene::new().with_atlas(Arc::clone(&atlas)))
        .recorded();
    assert_eq!(
        bound_view(&ui, 1),
        atlas_view(&atlas),
        "the atlas's view, once it has one"
    );
    assert_eq!(
        ui.device
            .destroyed()
            .iter()
            .filter(|destroyed| **destroyed == (GpuKind::BindGroup, fallback_group.0))
            .count(),
        1,
        "the bind group over the fallback is given back, once"
    );

    ui.record(&Scene::new().with_atlas(empty_atlas()))
        .recorded();
    assert_eq!(
        bound_view(&ui, 2),
        fallback_view,
        "the fallback is bound again"
    );
    assert_eq!(
        ui.device.made_of(GpuKind::Texture).len(),
        1,
        "and not made again: {:?}",
        ui.device.made()
    );

    ui.shut_down();
    assert_everything_released(&ui.device);
}

/// **A refusal while building over an empty atlas leaks nothing**: for every
/// object a first frame over an atlas without a texture asks for — the
/// fallback texture and its view among them — a device refusing it fails the
/// frame; the next frame draws its own glyphs; and the shutdown gives back
/// everything the lane made, once.
#[test]
fn a_refusal_while_building_over_an_empty_atlas_leaks_nothing() {
    let steps = {
        let mut ui = Ui::new();
        ui.record(&Scene::new().with_resident_text().with_atlas(empty_atlas()))
            .recorded();
        ui.device.asked()
    };

    for refused in 1..=steps {
        let mut ui = Ui::on(Ledger::refusing_the(refused));
        let scene = Scene::new().with_resident_text().with_atlas(empty_atlas());

        ui.record(&scene).failed();
        ui.device.stop_refusing();
        ui.record(&scene).recorded();

        assert_last_frame_draws_its_own_glyphs(&ui);
        ui.shut_down();
        assert_everything_released(&ui.device);
    }
}

/// **A failed frame leaves the next frame its own glyphs.** A refused
/// storage growth, and a refused bind group over a new atlas view, fail
/// their frame after its instances were computed: the next frame draws its
/// own text's glyphs, each once.
#[test]
fn a_failed_frame_leaves_the_next_frame_its_own_glyphs() {
    let resident = Scene::new().with_resident_text();
    let glyphs = Arc::clone(&resident.glyphs);
    let texts = resident.ui.texts.clone();
    let scene = || {
        Scene::new()
            .with_glyphs(Arc::clone(&glyphs))
            .with_texts(texts.clone())
    };
    let build = {
        let mut ui = Ui::new();
        ui.record(&scene()).recorded();
        ui.device.asked()
    };

    // The growth: the first object the second frame asks for.
    let mut ui = Ui::on(Ledger::refusing_the(build + 1));
    ui.record(&scene()).recorded();
    ui.record(&scene().with_node_count(4 * OLD_CAPACITY))
        .failed();
    ui.record(&scene()).recorded();
    assert_last_frame_draws_its_own_glyphs(&ui);

    // The bind group over a new atlas view: likewise.
    let mut ui = Ui::on(Ledger::refusing_the(build + 1));
    ui.record(&scene()).recorded();
    let moved = scene().with_atlas(super::fixtures::gpu_atlas());
    ui.record(&moved).failed();
    ui.record(&moved).recorded();
    assert_last_frame_draws_its_own_glyphs(&ui);
}
