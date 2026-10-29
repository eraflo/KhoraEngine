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

//! The UI lane hands its frame's text to the text renderer: every text
//! queued once, in the order the UI Flow sorted them (by layer), then
//! flushed into the UI pass's own encoder, over the quads, against the
//! frame's colour target at its size — whether or not the frame has a node.

use khora_core::math::{Extent2D, Vec2, Vec4};

use super::fixtures::{nth_node, nth_text, queued, sized, text, Scene, Ui, COLOR_VIEW};

/// **A frame of text and no node still flushes its text** — the lane's very
/// first frame: text does not wait for a quad to be drawn. The text is
/// queued, then flushed into the UI pass's encoder, against the frame's
/// colour target at its size.
#[test]
fn a_frame_with_text_and_no_ui_node_still_flushes_its_text() {
    let mut ui = Ui::new();
    let only = text(Vec2::new(12.0, 34.0), Vec2::new(40.0, 16.0), Vec4::ONE, 0);
    let scene = Scene::new()
        .with_nodes(Vec::new())
        .with_texts(vec![only.clone()])
        .into_target(sized(1384, 561));

    let recorded = ui.record(&scene);
    let pass = recorded.recorded();

    assert_eq!(
        ui.text.queued(),
        [queued(&only)],
        "the frame's text is queued"
    );
    let flushes = ui.text.flushes();
    assert_eq!(flushes.len(), 1, "flushed once: {flushes:?}");
    assert_eq!(
        flushes[0].encoder,
        Some(pass.command_buffer),
        "into the UI pass's encoder"
    );
    assert_eq!(flushes[0].color, COLOR_VIEW, "into the frame's colour");
    assert_eq!(
        flushes[0].size,
        Extent2D {
            width: 1384,
            height: 561
        },
        "at its size, not the surface's"
    );
    assert_eq!(flushes[0].texts, [queued(&only)]);
}

/// **The texts are queued in layer order**, each once, with its layout, its
/// position, its colour and its layer — in the order the UI Flow sorted them
/// by `z_index` — and all are flushed together.
#[test]
fn the_texts_are_queued_in_layer_order_with_their_layout_position_and_colour() {
    let mut ui = Ui::new();
    let texts = vec![
        text(
            Vec2::new(1.0, 2.0),
            Vec2::new(10.0, 8.0),
            Vec4::new(1.0, 0.0, 0.0, 1.0),
            -3,
        ),
        text(
            Vec2::new(3.0, 4.0),
            Vec2::new(20.0, 8.0),
            Vec4::new(0.0, 1.0, 0.0, 1.0),
            0,
        ),
        text(
            Vec2::new(5.0, 6.0),
            Vec2::new(30.0, 8.0),
            Vec4::new(0.0, 0.0, 1.0, 0.5),
            7,
        ),
    ];

    ui.record(&Scene::new().with_texts(texts.clone()))
        .recorded();

    let expected: Vec<_> = texts.iter().map(queued).collect();
    assert_eq!(ui.text.queued(), expected, "queued once each, in order");
    let flushes = ui.text.flushes();
    assert_eq!(flushes.len(), 1, "{flushes:?}");
    assert_eq!(flushes[0].texts, expected, "flushed together");
}

/// **The text is drawn over the quads**: the frame's quad pass is recorded
/// through the UI pass's encoder before the text is flushed into that same
/// encoder.
#[test]
fn the_text_is_flushed_into_the_ui_pass_after_the_quads() {
    let mut ui = Ui::new();
    let scene = Scene::new().with_node_count(2).with_text_count(2);

    let recorded = ui.record(&scene);
    let pass = recorded.recorded();

    let flushes = ui.text.flushes();
    assert_eq!(flushes.len(), 1, "{flushes:?}");
    assert_eq!(flushes[0].encoder, Some(pass.command_buffer));
    assert_eq!(
        flushes[0].passes_before,
        1,
        "the quad pass was recorded before the text: {:?}",
        ui.device.passes()
    );
    assert_eq!(ui.quad_pass(0).buffer, pass.command_buffer);
}

/// **A frame with quads flushes its text into the frame's target at its
/// size**, once, every text of the frame queued.
#[test]
fn a_frame_with_quads_flushes_its_text_into_the_frame_target_at_its_size() {
    let mut ui = Ui::new();
    let scene = Scene::new()
        .with_nodes(vec![nth_node(0), nth_node(1)])
        .with_texts(vec![nth_text(0), nth_text(1)])
        .into_target(sized(1920, 1080));

    ui.record(&scene).recorded();

    assert_eq!(
        ui.text.queued(),
        [queued(&nth_text(0)), queued(&nth_text(1))],
        "every text of the frame is queued"
    );
    let flushes = ui.text.flushes();
    assert_eq!(flushes.len(), 1, "the text is flushed once: {flushes:?}");
    assert_eq!(flushes[0].color, COLOR_VIEW);
    assert_eq!(
        flushes[0].size,
        Extent2D {
            width: 1920,
            height: 1080
        }
    );
}

/// **Successive frames flush text at each frame's target size**: a resized
/// target reaches the text renderer on the next frame.
#[test]
fn successive_frames_flush_text_at_each_frame_target_size() {
    let mut ui = Ui::new();

    ui.record(&Scene::new().into_target(sized(800, 600)))
        .recorded();
    ui.record(&Scene::new().into_target(sized(1384, 561)))
        .recorded();

    let sizes: Vec<Extent2D> = ui.text.flushes().iter().map(|flush| flush.size).collect();
    assert_eq!(
        sizes,
        [
            Extent2D {
                width: 800,
                height: 600
            },
            Extent2D {
                width: 1384,
                height: 561
            }
        ]
    );
    let encoders: Vec<_> = ui
        .text
        .flushes()
        .iter()
        .map(|flush| flush.encoder)
        .collect();
    assert_eq!(
        encoders,
        ui.device.opened().into_iter().map(Some).collect::<Vec<_>>(),
        "each frame flushes into its own encoder"
    );
}

/// **A refused text flush fails the frame**, which commits no pass; the
/// next frame, the renderer willing, flushes its own text only.
#[test]
fn a_refused_text_flush_fails_the_frame() {
    let mut ui = Ui::new();
    ui.text.fail_flushes();

    ui.record(&Scene::new()).failed();

    ui.text.stop_failing();
    ui.record(&Scene::new()).recorded();
    let flushes = ui.text.flushes();
    assert_eq!(
        flushes.last().map(|flush| flush.texts.clone()),
        Some(vec![queued(&nth_text(0))]),
        "the next frame flushes its own text: {flushes:?}"
    );
}
