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

//! The atlas bind group: the UI samples the image atlas through a bind group
//! over the atlas's view and the lane's sampler, kept while the view is the
//! same — new images in the atlas keep its view — and replaced, the old one
//! given back, when the atlas texture was made again. The lane only reads
//! the atlas: it never makes nor destroys the atlas's texture or view.

use super::fixtures::{atlas_view, gpu_atlas, Bound, GpuKind, Scene, Ui};

/// **The atlas bind group binds the atlas's view and a sampler** the lane
/// made, at group 2.
#[test]
fn the_atlas_bind_group_binds_the_atlas_view_and_a_sampler() {
    let mut ui = Ui::new();
    let scene = Scene::new();

    ui.record(&scene).recorded();

    let group = ui.quad_pass(0).bind_group(2).expect("the atlas at group 2");
    assert_eq!(
        ui.device.bound_at(group, 0),
        Some(Bound::View(atlas_view(&scene.atlas))),
        "the atlas's view at binding 0"
    );
    match ui.device.bound_at(group, 1) {
        Some(Bound::Sampler(sampler)) => assert!(
            ui.device.find(GpuKind::Sampler, sampler.0).is_some(),
            "a sampler the lane made"
        ),
        other => panic!("a sampler at binding 1, not {other:?}"),
    }
}

/// **The atlas bind group is kept while the atlas view is unchanged**:
/// three frames over one atlas — the second with an image placed since —
/// bind one bind group, made once.
#[test]
fn the_atlas_bind_group_is_kept_while_the_atlas_view_is_unchanged() {
    let mut ui = Ui::new();
    let scene = Scene::new();

    ui.record(&scene).recorded();
    let first = ui.quad_pass(0).bind_group(2).expect("the atlas");
    let bind_groups = ui.device.made_of(GpuKind::BindGroup).len();
    scene.atlas.insert_rect(
        khora_core::asset::AssetUUID::new_v5("placed since"),
        super::fixtures::rect(0.0, 0.0, 0.5, 0.5),
    );
    ui.record(&scene).recorded();
    ui.record(&scene).recorded();

    for frame in 1..3 {
        assert_eq!(
            ui.quad_pass(frame).bind_group(2),
            Some(first),
            "frame {frame} binds the same atlas bind group"
        );
    }
    assert_eq!(
        ui.device.made_of(GpuKind::BindGroup).len(),
        bind_groups,
        "no bind group is made again"
    );
    assert!(!ui.device.is_destroyed(GpuKind::BindGroup, first.0));
}

/// **A new atlas view replaces the atlas bind group.** The atlas texture was
/// made again: the next frame binds a new bind group over the new view, and
/// the old one is given back, once; the frame after keeps the new one.
#[test]
fn a_new_atlas_view_replaces_the_atlas_bind_group() {
    let mut ui = Ui::new();
    let first_atlas = gpu_atlas();
    let second_atlas = gpu_atlas();
    assert_ne!(atlas_view(&first_atlas), atlas_view(&second_atlas));

    ui.record(&Scene::new().with_atlas(first_atlas)).recorded();
    let first = ui.quad_pass(0).bind_group(2).expect("the atlas");
    ui.record(&Scene::new().with_atlas(second_atlas.clone()))
        .recorded();
    let second = ui.quad_pass(1).bind_group(2).expect("the atlas");
    ui.record(&Scene::new().with_atlas(second_atlas.clone()))
        .recorded();

    assert_ne!(second, first, "a new bind group");
    assert_eq!(
        ui.device.bound_at(second, 0),
        Some(Bound::View(atlas_view(&second_atlas))),
        "over the new view"
    );
    assert!(
        ui.device.is_destroyed(GpuKind::BindGroup, first.0),
        "the old bind group is given back"
    );
    assert_eq!(
        ui.device
            .destroyed()
            .iter()
            .filter(|destroyed| **destroyed == (GpuKind::BindGroup, first.0))
            .count(),
        1,
        "once"
    );
    assert_eq!(ui.quad_pass(2).bind_group(2), Some(second), "and kept");
    assert!(!ui.device.is_destroyed(GpuKind::BindGroup, second.0));
}
