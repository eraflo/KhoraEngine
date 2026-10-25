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

//! A UI frame with quads hands its text to the text renderer with the frame's
//! colour target and size; a frame on a target without area hands it nothing.

use super::*;
use khora_data::ui::ExtractedUiNode;

/// A plain UI quad at `pos`.
fn node_at(pos: Vec2) -> ExtractedUiNode {
    ExtractedUiNode {
        pos,
        size: Vec2::new(100.0, 20.0),
        color: None,
        border: None,
        image: None,
        z_index: 0,
    }
}

#[test]
fn a_frame_with_quads_flushes_its_text_into_the_frame_target_at_its_size() {
    let (lane, device) = initialised_lane();
    let text = Arc::new(RecordingTextRenderer::default());
    let scene = UiScene {
        nodes: vec![node_at(Vec2::new(0.0, 0.0)), node_at(Vec2::new(0.0, 30.0))],
        texts: vec![text_at(Vec2::new(4.0, 2.0)), text_at(Vec2::new(4.0, 32.0))],
    };
    let target = extent(1920, 1080);

    run_frame(&lane, &device, &scene, &text, target);

    assert_eq!(
        *text.queued.lock().expect("queued lock"),
        [Vec2::new(4.0, 2.0), Vec2::new(4.0, 32.0)],
        "every text of the frame is queued"
    );
    assert_eq!(
        *text.flushes.lock().expect("flushes lock"),
        [(COLOR_TARGET, target)],
        "the text is flushed once, into the frame's colour target, at its size"
    );
}

#[test]
fn successive_frames_flush_text_at_each_frame_target_size() {
    let (lane, device) = initialised_lane();
    let text = Arc::new(RecordingTextRenderer::default());
    let scene = UiScene {
        nodes: vec![node_at(Vec2::new(0.0, 0.0))],
        texts: vec![text_at(Vec2::new(4.0, 2.0))],
    };

    run_frame(&lane, &device, &scene, &text, extent(800, 600));
    run_frame(&lane, &device, &scene, &text, extent(1384, 561));

    assert_eq!(
        *text.flushes.lock().expect("flushes lock"),
        [
            (COLOR_TARGET, extent(800, 600)),
            (COLOR_TARGET, extent(1384, 561))
        ],
        "a resized target reaches the text renderer on the next frame"
    );
}

#[test]
fn a_frame_on_a_target_without_area_neither_queues_nor_flushes_text() {
    let (lane, device) = initialised_lane();
    let text = Arc::new(RecordingTextRenderer::default());
    let scene = UiScene {
        nodes: vec![node_at(Vec2::new(0.0, 0.0))],
        texts: vec![text_at(Vec2::new(4.0, 2.0))],
    };

    for target in [extent(0, 600), extent(800, 0), extent(0, 0)] {
        run_frame(&lane, &device, &scene, &text, target);
    }

    assert!(
        text.queued.lock().expect("queued lock").is_empty(),
        "no text is queued for a target without area, so none is left for the next frame"
    );
    assert!(
        text.flushes.lock().expect("flushes lock").is_empty(),
        "nothing is flushed into a target without area"
    );
}
