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

//! The glyph atlas of `StandardTextRenderer` keeps every glyph a frame needs:
//! a text drawn at many sizes over a session never runs it out.

use super::*;

/// Queues `text` laid out in `face` at `size`.
fn queue_sized(renderer: &StandardTextRenderer, text: &str, face: FontFace<'_>, size: f32) {
    let layout = renderer.layout_text(text, face, size, None);
    renderer.queue_text(layout.as_ref(), Vec2::new(0.0, 0.0), Vec4::ONE, 0);
}

#[test]
fn a_text_whose_size_animates_keeps_every_glyph_drawn() {
    let renderer = renderer();
    let device = RecordingDevice::default();
    let alphabet = glyphs(26);

    // A label growing from 16 px to 64 px in tenth-of-a-pixel steps: eight
    // seconds of a 60 Hz tween, one size a frame.
    for step in 0..480 {
        let size = 16.0 + step as f32 * 0.1;
        queue_sized(&renderer, &alphabet, FontFace::BuiltIn, size);
        let passes = flush(&renderer, &device, extent(1920, 1080));
        assert_eq!(
            indices_drawn(&passes),
            26 * 6,
            "frame {step} (size {size}) draws all 26 glyphs"
        );
    }
}
