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

//! The shaper's glyphs keyed with `GlyphKey::new` reach the rasteriser: the
//! two backends agree on glyph ids and sizes. (`GlyphKey`'s quantisation is
//! pinned next to the type, in `khora-core`.)

use khora_core::ui::text::{GlyphKey, GlyphRasterizer};

use super::{built_in, key, shaped, H};
use crate::ui::text::{ParleyShaper, SwashRasterizer};

#[test]
fn every_letter_of_a_shaped_word_rasterises_from_its_key() {
    let shaper = ParleyShaper::new();
    let rasterizer = SwashRasterizer::new();
    let size_px = 16.2;
    let text = shaped(&shaper, "Hello", size_px);
    assert_eq!(text.glyphs.len(), 5);

    for g in &text.glyphs {
        let key = GlyphKey::new(built_in(), g.id, size_px, g.x);
        assert_eq!(
            key.size_px(),
            16.25,
            "16.2 px is keyed at the nearest quarter"
        );
        let image = rasterizer.rasterize(&key);
        assert!(
            image.is_some_and(|i| i.pixels.iter().any(|&p| p > 0)),
            "glyph {} at x = {} rasterised into nothing",
            g.id,
            g.x
        );
    }
}

#[test]
fn a_key_rasterises_at_its_quantised_size() {
    let rasterizer = SwashRasterizer::new();
    let near = GlyphKey::new(built_in(), H, 24.1, 0.0);
    assert_eq!(
        rasterizer.rasterize(&near),
        rasterizer.rasterize(&key(built_in(), H, 24.0, 0)),
        "24.1 px draws as 24 px"
    );
}
