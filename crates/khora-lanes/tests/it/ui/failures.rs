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

//! What the UI lane does when the device refuses something it asks for: the
//! frame fails and commits nothing, what was built before the refusal is
//! kept or given back — never leaked, never destroyed twice — and the next
//! frame builds what is missing and draws its own glyphs.

use std::sync::Arc;

use khora_core::lane::erased::RunOutcome;
use khora_core::math::{Vec2, Vec4};

use super::fixtures::{
    assert_everything_released, glyph_text, Glyphs, GpuKind, Ledger, Scene, Ui, OLD_CAPACITY,
    RESIDENT_GLYPHS, UI_SHADER, UI_TEXT_SHADER,
};

/// How many objects a whole build of the lane asks the device for, over a
/// node and a resident text: the atlas bind group, the glyphs' storage and
/// bind groups, and the fallback the mask-only glyph atlas needs included.
fn objects_of_a_build() -> usize {
    let mut ui = Ui::new();
    ui.record(&Scene::new().with_resident_text()).recorded();
    ui.device.asked()
}

/// **A refusal halfway through the UI's build leaks nothing.** For every
/// step of the build, a lane whose device refuses that step: the frame fails
/// and commits nothing; the next frame, the device willing, builds, draws,
/// and draws its own text's glyphs, each once; and after a shutdown nothing
/// the lane ever made is left alive or was destroyed twice.
#[test]
fn a_refusal_halfway_through_the_ui_build_leaks_nothing() {
    let steps = objects_of_a_build();
    assert!(steps > 1, "a build makes several objects: {steps}");

    for refused in 1..=steps {
        let mut ui = Ui::on(Ledger::refusing_the(refused));
        let scene = Scene::new().with_resident_text();

        let failed = ui.record(&scene);
        assert!(
            matches!(failed.run.outcome, RunOutcome::Failed(_)),
            "refusing object {refused} of {steps} fails the frame: {:?}",
            failed.run.outcome
        );
        assert!(failed.pass.is_none(), "a failed frame commits no pass");

        ui.device.stop_refusing();
        ui.record(&scene).recorded();
        assert!(
            !ui.device.passes().is_empty(),
            "after refusing object {refused}, the next frame draws"
        );
        assert_eq!(
            ui.last_glyphs().len(),
            RESIDENT_GLYPHS,
            "after refusing object {refused}, the next frame draws its own glyphs"
        );
        ui.shut_down();
        assert_everything_released(&ui.device);
    }
}

/// **A refused pipeline keeps the UI's buffers for the retry.** A refused
/// pipeline fails the frame twice; once it is allowed, the lane draws with
/// it, having built its buffers no more often than one build does; its
/// shutdown releases everything.
#[test]
fn a_refused_ui_pipeline_keeps_the_buffers_for_the_retry() {
    let buffers_of_a_build = {
        let mut ui = Ui::new();
        ui.record(&Scene::new()).recorded();
        ui.device.made_of(GpuKind::Buffer).len()
    };
    let mut ui = Ui::new();
    let scene = Scene::new();
    ui.pipelines.refuse(UI_SHADER);

    ui.record(&scene).failed();
    ui.record(&scene).failed();
    ui.pipelines.stop_refusing();
    ui.record(&scene).recorded();

    let made = ui.device.made_of(GpuKind::Buffer);
    assert_eq!(
        made.len(),
        buffers_of_a_build,
        "the retries build the buffers no more often than one build: {made:?}"
    );
    let built = ui.pipelines.of(UI_SHADER);
    assert_eq!(built.len(), 1, "built once it is allowed");
    assert_eq!(ui.quad_pass(0).pipelines, [built[0].id]);
    ui.shut_down();
    assert_everything_released(&ui.device);
}

/// `count` 'A's at whole-pixel pens, 64 a line, resident in an atlas of
/// their own, as one white text at the origin.
fn many_resident_glyphs(count: usize) -> Scene {
    let mut glyphs = Glyphs::new();
    let shaped = glyphs.resident(
        &(0..count)
            .map(|i| (65, (i % 64) as f32 * 12.0, 12.0 + (i / 64) as f32 * 8.0))
            .collect::<Vec<_>>(),
    );
    Scene::new()
        .with_nodes(Vec::new())
        .with_glyphs(Arc::clone(&glyphs.atlas))
        .with_texts(vec![glyph_text(Vec2::ZERO, Vec4::ONE, 0, &shaped)])
}

/// **A refused glyph storage growth fails the frame, and the next frame
/// draws every glyph.** After a frame of a few glyphs, a frame of four
/// thousand whose larger glyph storage the device refuses fails and commits
/// nothing, its level still quoting the buffers the lane holds; the next
/// frame grows the storage and draws all of them; the shutdown releases
/// everything, once.
#[test]
fn a_refused_glyph_storage_growth_fails_the_frame_and_the_next_draws_every_glyph() {
    const MANY: usize = 4 * OLD_CAPACITY;
    let few = Scene::new().with_resident_text().with_nodes(Vec::new());
    let build = {
        let mut ui = Ui::new();
        ui.record(&few).recorded();
        ui.device.asked()
    };
    let many = many_resident_glyphs(MANY);
    let mut ui = Ui::on(Ledger::refusing_the(build + 1));
    ui.record(&few).recorded();

    ui.record(&many).failed();
    assert_eq!(
        ui.quoted_memory(),
        ui.device.live_bytes(),
        "the level quotes what the lane holds after the refusal: {:?}",
        ui.device.live_buffers()
    );

    ui.record(&many).recorded();
    assert_eq!(ui.last_glyphs().len(), MANY, "every glyph is drawn");
    assert_eq!(
        ui.quoted_memory(),
        ui.device.live_bytes(),
        "the level quotes the grown glyph storage"
    );
    ui.shut_down();
    assert_everything_released(&ui.device);
}

/// **A refused glyph bind group over a new glyph atlas view fails the frame,
/// and the next frame binds the new view.** After a frame over one glyph
/// atlas, a frame over another — whose group 2 the device refuses — fails;
/// the next binds that atlas's mask view and draws its glyphs; the shutdown
/// releases everything, once.
#[test]
fn a_refused_glyph_bind_group_fails_the_frame_and_the_next_binds_the_new_view() {
    let first = Scene::new().with_resident_text();
    let build = {
        let mut ui = Ui::new();
        ui.record(&first).recorded();
        ui.device.asked()
    };
    let moved = Scene::new().with_resident_text();
    let mut ui = Ui::on(Ledger::refusing_the(build + 1));
    ui.record(&first).recorded();

    ui.record(&moved).failed();
    ui.record(&moved).recorded();

    assert_eq!(ui.last_glyphs().len(), RESIDENT_GLYPHS, "its own glyphs");
    let group = ui.glyph_group(ui.device.opened().len() - 1, 2);
    assert_eq!(
        Some(ui.view_at(group, 0)),
        moved.glyphs.views().0,
        "group 2 binds the new atlas's mask view"
    );
    ui.shut_down();
    assert_everything_released(&ui.device);
}

/// **A refused glyph pipeline fails the frame and keeps the buffers for the
/// retry.** Refusing `ui_text` fails the frame twice; once it is allowed,
/// the lane draws its glyphs with it, having built its buffers no more often
/// than one build does; its shutdown releases everything.
#[test]
fn a_refused_glyph_pipeline_fails_the_frame_and_keeps_the_buffers_for_the_retry() {
    let scene = Scene::new().with_resident_text();
    let buffers_of_a_build = {
        let mut ui = Ui::new();
        ui.record(&scene).recorded();
        ui.device.made_of(GpuKind::Buffer).len()
    };
    let mut ui = Ui::new();
    ui.pipelines.refuse(UI_TEXT_SHADER);

    ui.record(&scene).failed();
    ui.record(&scene).failed();
    ui.pipelines.stop_refusing();
    ui.record(&scene).recorded();

    assert_eq!(
        ui.device.made_of(GpuKind::Buffer).len(),
        buffers_of_a_build,
        "the retries build the buffers no more often than one build"
    );
    let built = ui.pipelines.of(UI_TEXT_SHADER);
    assert_eq!(built.len(), 1, "built once it is allowed: {built:?}");
    let draws = ui.glyph_draws();
    assert!(
        !draws.is_empty() && draws.iter().all(|draw| draw.pipeline == Some(built[0].id)),
        "the glyphs are drawn with it: {draws:?}"
    );
    ui.shut_down();
    assert_everything_released(&ui.device);
}
