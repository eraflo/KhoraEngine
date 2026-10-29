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

//! The projection the UI lane uploads follows the target it draws into: UI
//! pixels are pixels of that image — the window, or the editor's viewport —
//! origin top-left, y down, never the surface's size.

use khora_core::math::Extent2D;

use super::fixtures::{assert_projects_onto_clip_space, floats, sized, Scene, Ui};

/// The projection the `n`-th quad pass drew with, as sixteen floats.
fn projection(ui: &Ui, n: usize) -> Vec<f32> {
    let block = ui.buffer_at(n, 0);
    let bytes = ui.device.last_write_at(block, 0);
    assert_eq!(bytes.len(), 64, "a 4×4 matrix of floats: {bytes:?}");
    floats(&bytes)
}

/// Draws one frame into a `width`×`height` target, then asserts its
/// projection maps that target onto clip space.
fn assert_frame_projects(width: u32, height: u32) {
    let mut ui = Ui::new();

    ui.record(&Scene::new().into_target(sized(width, height)))
        .recorded();

    assert_projects_onto_clip_space(&projection(&ui, 0), Extent2D { width, height });
}

/// **The UI projection is uploaded for the target's size**: 1024×768 and
/// 1384×561 — never the ledger's 3×3 surface.
#[test]
fn the_ui_projection_is_uploaded_for_the_targets_size() {
    assert_frame_projects(1024, 768);
    assert_frame_projects(1384, 561);
}

/// **A one-pixel target projects its pixel onto the whole clip space.**
#[test]
fn a_one_pixel_target_projects_its_pixel_onto_the_whole_clip_space() {
    assert_frame_projects(1, 1);
}

/// **The largest targets still project their corners onto clip space**,
/// with only finite numbers.
#[test]
fn the_largest_targets_still_project_their_corners_onto_clip_space() {
    assert_frame_projects(16_384, 16_384);
    assert_frame_projects(32_768, 1);
    assert_frame_projects(u32::MAX, u32::MAX);
}

/// **A resized target uploads its new projection**: the formats do not
/// change, so nothing is built again, but the next frame's projection maps
/// the new size.
#[test]
fn a_resized_target_uploads_its_new_projection() {
    let mut ui = Ui::new();
    ui.record(&Scene::new().into_target(sized(800, 600)))
        .recorded();
    let built = ui.device.made().len();

    ui.record(&Scene::new().into_target(sized(1384, 561)))
        .recorded();

    assert_projects_onto_clip_space(
        &projection(&ui, 1),
        Extent2D {
            width: 1384,
            height: 561,
        },
    );
    assert_eq!(ui.device.made().len(), built, "a resize builds nothing");
}
