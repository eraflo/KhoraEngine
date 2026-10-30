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

//! `SwashRasterizer` turns one glyph key into one image.

use khora_core::ui::text::{GlyphImage, GlyphKey, GlyphKind, GlyphRasterizer};

use super::{built_in, fresh_id, key, A, H, INTER_GLYPH_COUNT, P, SPACE};
use crate::ui::text::SwashRasterizer;

/// The built-in font's `glyph` at `size_px`, bin 0; panics if none.
fn image(rasterizer: &SwashRasterizer, glyph: u16, size_px: f32) -> GlyphImage {
    rasterizer
        .rasterize(&key(built_in(), glyph, size_px, 0))
        .unwrap_or_else(|| panic!("glyph {glyph} at {size_px} px rasterised into nothing"))
}

#[test]
fn a_letter_rasterises_into_a_non_empty_mask() {
    let h = image(&SwashRasterizer::new(), H, 16.0);

    assert_eq!(h.kind, GlyphKind::Mask);
    assert!(h.width > 0 && h.height > 0, "{}x{}", h.width, h.height);
    assert_eq!(
        h.pixels.len(),
        (h.width * h.height) as usize,
        "a mask holds one byte per pixel"
    );
    assert!(h.pixels.iter().any(|&p| p > 0), "the mask covers something");
}

#[test]
fn a_bigger_size_gives_a_bigger_image() {
    let rasterizer = SwashRasterizer::new();
    let small = image(&rasterizer, H, 16.0);
    let large = image(&rasterizer, H, 32.0);

    assert!(
        large.width > small.width,
        "{} vs {}",
        small.width,
        large.width
    );
    assert!(
        large.height > small.height,
        "{} vs {}",
        small.height,
        large.height
    );
    assert!(
        large.height + 2 >= 2 * small.height,
        "twice the size is about twice as tall: {} vs {}",
        small.height,
        large.height
    );
}

#[test]
fn a_capital_sits_on_the_baseline_and_a_descender_drops_below_it() {
    let rasterizer = SwashRasterizer::new();

    let h = image(&rasterizer, H, 32.0);
    assert!(h.top > 0, "H rises above the baseline, top = {}", h.top);
    let bottom = h.top - h.height as i32;
    assert!(
        bottom.abs() <= 1,
        "H's bottom edge is on the baseline, got {bottom}"
    );

    let p = image(&rasterizer, P, 32.0);
    let bottom = p.top - p.height as i32;
    assert!(
        bottom < -2,
        "p's descender drops below the baseline, got {bottom}"
    );
}

#[test]
fn a_space_rasterises_into_nothing() {
    let rasterizer = SwashRasterizer::new();
    for bin in 0..=3 {
        assert_eq!(
            rasterizer.rasterize(&key(built_in(), SPACE, 16.0, bin)),
            None,
            "bin {bin}"
        );
    }
}

#[test]
fn every_subpixel_bin_of_a_letter_rasterises() {
    let rasterizer = SwashRasterizer::new();
    for bin in 0..=3 {
        let a = rasterizer.rasterize(&key(built_in(), A, 16.0, bin));
        assert!(
            a.is_some_and(|a| a.pixels.iter().any(|&p| p > 0)),
            "bin {bin}"
        );
    }
}

#[test]
fn two_subpixel_bins_of_one_glyph_rasterise_differently() {
    let rasterizer = SwashRasterizer::new();
    let whole = rasterizer.rasterize(&key(built_in(), H, 16.0, 0));
    let half = rasterizer.rasterize(&key(built_in(), H, 16.0, 2));
    assert!(whole.is_some() && half.is_some());
    assert_ne!(
        whole, half,
        "H's stems moved half a pixel right cover other pixels"
    );
}

#[test]
fn the_same_key_rasterises_into_the_same_pixels() {
    let rasterizer = SwashRasterizer::new();
    let key = key(built_in(), A, 18.25, 3);
    let first = rasterizer.rasterize(&key);
    assert!(first.is_some());
    let _other = rasterizer.rasterize(&super::key(built_in(), H, 40.0, 1));
    assert_eq!(rasterizer.rasterize(&key), first, "one rasteriser, twice");
    assert_eq!(
        SwashRasterizer::new().rasterize(&key),
        first,
        "two rasterisers"
    );
}

#[test]
fn an_unknown_font_rasterises_into_nothing() {
    assert_eq!(
        SwashRasterizer::new().rasterize(&key(fresh_id(), H, 16.0, 0)),
        None
    );
}

#[test]
fn a_glyph_the_font_does_not_have_rasterises_into_nothing() {
    let rasterizer = SwashRasterizer::new();
    for glyph in [INTER_GLYPH_COUNT, INTER_GLYPH_COUNT + 100, u16::MAX] {
        assert_eq!(
            rasterizer.rasterize(&key(built_in(), glyph, 16.0, 0)),
            None,
            "glyph {glyph}"
        );
    }
}

/// A key whose size quantised to zero (a size under ⅛ px, or a negative or
/// NaN one through `GlyphKey::new`) draws nothing. swash reads a size of 0 as
/// "unscaled", so passing it through draws the glyph in font units — Inter's
/// `H` comes back about 1162 × 1490 px.
#[test]
fn a_key_of_size_zero_rasterises_into_nothing() {
    let rasterizer = SwashRasterizer::new();
    let zero = GlyphKey {
        size_q: 0,
        ..key(built_in(), H, 16.0, 0)
    };
    let image = rasterizer.rasterize(&zero);
    assert_eq!(
        image.as_ref().map(|i| (i.width, i.height)),
        None,
        "a zero-size key drew an image"
    );
    for size_px in [0.1, -16.0, f32::NAN] {
        let key = GlyphKey::new(built_in(), H, size_px, 0.0);
        assert_eq!(
            rasterizer.rasterize(&key).map(|i| (i.width, i.height)),
            None,
            "GlyphKey::new at {size_px} px drew an image"
        );
    }
}

/// The default font is variable (weights 100 to 900). Built with no
/// variation, the rasteriser must draw its default instance — Regular,
/// `wght` 400, the weight the shaper's advances are measured at — and not a
/// thinner or bolder master.
#[test]
fn the_default_font_rasterises_at_its_regular_weight() {
    use swash::scale::{Render, ScaleContext, Source};
    use swash::zeno::Format;

    fn at_weight(weight: f32) -> Vec<u8> {
        let font = swash::FontRef::from_index(crate::ui::text::DEFAULT_FONT, 0)
            .expect("the default font reads");
        let mut context = ScaleContext::new();
        let mut scaler = context
            .builder(font)
            .size(32.0)
            .hint(false)
            .variations(&[("wght", weight)])
            .build();
        Render::new(&[Source::Outline])
            .format(Format::Alpha)
            .render(&mut scaler, H)
            .expect("H has an outline")
            .data
    }

    let drawn = image(&SwashRasterizer::new(), H, 32.0).pixels;
    assert_eq!(drawn, at_weight(400.0), "H is drawn at wght 400");
    assert_ne!(drawn, at_weight(100.0), "H is not drawn Thin");
}
