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

//! UI text: shaping a string into positioned glyphs, and rasterising one glyph
//! into an image an atlas can hold.
//!
//! Two contracts, each implemented by a backend in `khora-infra`:
//! [`TextShaper`] turns a string, a font and a size into [`ShapedText`];
//! [`GlyphRasterizer`] turns a [`GlyphKey`] into a [`GlyphImage`]. Fonts are
//! named by their asset id ([`AssetUUID`]); `Font::BUILT_IN`
//! (`crate::asset::font::Font::BUILT_IN`) is registered by every backend at
//! construction with the engine's default font.

use std::sync::Arc;

use crate::asset::AssetUUID;
use crate::math::Vec2;

/// The family a registered font file declares, read from the file itself.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FontFamily {
    /// The family name, as the font file's name table spells it.
    pub name: String,
}

/// Why a font could not be registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontError {
    /// The bytes are not a font file the backend can read.
    Unreadable,
}

impl std::fmt::Display for FontError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreadable => f.write_str("the font data is not a readable font file"),
        }
    }
}

impl std::error::Error for FontError {}

/// Why a text could not be shaped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShapeError {
    /// No font is registered under this id.
    UnknownFont(AssetUUID),
}

impl std::fmt::Display for ShapeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownFont(id) => write!(f, "no font is registered under {id}"),
        }
    }
}

impl std::error::Error for ShapeError {}

/// What to shape: a string, in one registered font, at one size.
#[derive(Debug, Clone, Copy)]
pub struct ShapeRequest<'a> {
    /// The text. A `'\n'` opens a new line.
    pub text: &'a str,
    /// The id the font was registered under.
    pub font: AssetUUID,
    /// The font size, in pixels per em.
    pub size_px: f32,
    /// The width, in pixels, past which a line breaks at a break opportunity
    /// (a space). `None` breaks only at `'\n'`.
    pub max_width: Option<f32>,
}

/// One glyph of a shaped text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapedGlyph {
    /// The glyph's id in its font (the font's own glyph index, as a
    /// [`GlyphRasterizer`] reads it).
    pub id: u16,
    /// The pen position's x, in pixels from the text's left edge.
    pub x: f32,
    /// The pen position's y, in pixels from the text's top edge, on the
    /// glyph's line baseline (y grows downwards).
    pub y: f32,
}

/// A shaped text: its glyphs at their pen positions, and the box they fill.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapedText {
    /// The glyphs, in visual order, line after line.
    pub glyphs: Vec<ShapedGlyph>,
    /// The text's width (its widest line) and height (all its lines), in
    /// pixels.
    pub size: Vec2,
    /// The number of lines the text was laid out on.
    pub lines: usize,
}

/// Shapes text with registered fonts.
///
/// # Locks
///
/// A shaper is reached from concurrent waves (RULES §5). Each implementation
/// holds **one leaf lock**, only around its own font and shaping state (its
/// font collection, its shaping context), and never calls out of the
/// implementation while it holds it. Every method can therefore be called from
/// many threads at once without deadlocking.
pub trait TextShaper: Send + Sync {
    /// Registers the font file `data` under `id` and returns the family the
    /// file declares. Registering an `id` again replaces its font (a hot
    /// reload): later shaping under `id` uses the new data. A refused
    /// registration leaves `id` with no font — never the file it held before,
    /// which is no longer the one its owner holds.
    fn register_font(&self, id: AssetUUID, data: Arc<[u8]>) -> Result<FontFamily, FontError>;

    /// Shapes `request.text` with the font registered under `request.font`.
    /// Refused with [`ShapeError::UnknownFont`] if no font is registered under
    /// that id.
    fn shape(&self, request: &ShapeRequest<'_>) -> Result<ShapedText, ShapeError>;
}

/// Whether a glyph image is a coverage mask or a colour image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GlyphKind {
    /// A coverage mask: one byte per pixel, the glyph's alpha. Tinted with the
    /// text's colour when drawn.
    Mask,
    /// A colour image (an emoji, a colour font): four bytes per pixel, RGBA,
    /// straight (not premultiplied) alpha.
    Color,
}

/// One glyph, rasterised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphImage {
    /// The image's width, in pixels.
    pub width: u32,
    /// The image's height, in pixels.
    pub height: u32,
    /// The image's left edge, in pixels right of the pen position.
    pub left: i32,
    /// The image's top edge, in pixels **above** the baseline (positive up).
    pub top: i32,
    /// What the pixels hold.
    pub kind: GlyphKind,
    /// The pixels, rows top to bottom: `width * height` bytes for a
    /// [`GlyphKind::Mask`], `width * height * 4` for a [`GlyphKind::Color`].
    pub pixels: Vec<u8>,
}

/// One glyph image as an atlas keys it: a font, a glyph, a quantised size and
/// a subpixel bin on x.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct GlyphKey {
    /// The id the font was registered under.
    pub font: AssetUUID,
    /// The glyph's id in its font.
    pub glyph: u16,
    /// The size in quarter pixels: `round(size_px * 4)`.
    pub size_q: u32,
    /// The subpixel bin of the pen's x (`0..=3`): its fractional part rounded
    /// to the nearest quarter pixel, a full pixel wrapping to bin 0.
    pub subpixel_x: u8,
}

impl GlyphKey {
    /// The key of `glyph` of `font` drawn at `size_px` with its pen at `x`.
    ///
    /// The size is quantised to a quarter pixel (`size_q = round(size_px *
    /// 4)`). The pen's x keeps only its fractional part, rounded to one of four
    /// bins (`0..=3`, bin `k` standing for `k / 4` px); a fraction that rounds
    /// to a full pixel wraps to bin 0. Two pen positions a whole number of
    /// pixels apart, left of the origin included, share a bin.
    pub fn new(font: AssetUUID, glyph: u16, size_px: f32, x: f32) -> Self {
        let fraction = x - x.floor();
        Self {
            font,
            glyph,
            size_q: (size_px.max(0.0) * 4.0).round() as u32,
            subpixel_x: ((fraction * 4.0).round() as u8) % 4,
        }
    }

    /// The size this key stands for, in pixels: `size_q / 4`.
    pub fn size_px(&self) -> f32 {
        self.size_q as f32 / 4.0
    }

    /// The pen offset this key's subpixel bin stands for, in pixels
    /// (`0.0`, `0.25`, `0.5` or `0.75`).
    pub fn subpixel_offset(&self) -> f32 {
        f32::from(self.subpixel_x) / 4.0
    }
}

/// Rasterises glyphs of registered fonts.
///
/// # Locks
///
/// A rasteriser is reached from concurrent waves (RULES §5). Each
/// implementation holds **one leaf lock**, only around its own font and
/// rasterising state (its fonts, its scale context), and never calls out of the
/// implementation while it holds it. Every method can therefore be called from
/// many threads at once without deadlocking.
pub trait GlyphRasterizer: Send + Sync {
    /// Registers the font file `data` under `id`. Registering an `id` again
    /// replaces its font (a hot reload); a refused registration leaves `id`
    /// with no font.
    fn register_font(&self, id: AssetUUID, data: Arc<[u8]>) -> Result<(), FontError>;

    /// Rasterises the glyph `key` names, offset right by its subpixel bin.
    /// `None` when the font is not registered, the font has no such glyph, or
    /// the glyph draws nothing (a space).
    fn rasterize(&self, key: &GlyphKey) -> Option<GlyphImage>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font() -> AssetUUID {
        AssetUUID::from_bytes(*b"khora:test:glyph")
    }

    fn bin(x: f32) -> u8 {
        GlyphKey::new(font(), 7, 16.0, x).subpixel_x
    }

    fn size_q(size_px: f32) -> u32 {
        GlyphKey::new(font(), 7, size_px, 0.0).size_q
    }

    #[test]
    fn a_glyph_key_keeps_its_font_and_glyph() {
        let key = GlyphKey::new(font(), 42, 16.0, 0.0);
        assert_eq!(key.font, font());
        assert_eq!(key.glyph, 42);
    }

    #[test]
    fn a_glyph_key_quantises_the_size_to_a_quarter_pixel() {
        assert_eq!(size_q(16.0), 64);
        assert_eq!(size_q(16.25), 65);
        assert_eq!(size_q(16.1), 64, "16.1 px rounds down to 16 px");
        assert_eq!(size_q(16.2), 65, "16.2 px rounds up to 16.25 px");
        assert_eq!(size_q(0.5), 2);
        assert_eq!(size_q(100.0), 400);
    }

    #[test]
    fn sizes_within_one_quarter_pixel_share_a_key() {
        assert_eq!(
            GlyphKey::new(font(), 7, 16.02, 0.0),
            GlyphKey::new(font(), 7, 15.98, 0.0)
        );
        assert_ne!(
            GlyphKey::new(font(), 7, 16.0, 0.0),
            GlyphKey::new(font(), 7, 16.25, 0.0)
        );
    }

    #[test]
    fn a_glyph_key_gives_back_its_quantised_size() {
        for size in [8.0, 12.5, 16.0, 16.25, 31.75, 64.0] {
            assert_eq!(GlyphKey::new(font(), 7, size, 0.0).size_px(), size);
        }
        assert_eq!(GlyphKey::new(font(), 7, 16.2, 0.0).size_px(), 16.25);
        assert_eq!(GlyphKey::new(font(), 7, 16.1, 0.0).size_px(), 16.0);
    }

    #[test]
    fn a_glyph_key_bins_the_pen_x_into_quarters() {
        assert_eq!(bin(0.0), 0);
        assert_eq!(bin(0.25), 1);
        assert_eq!(bin(0.5), 2);
        assert_eq!(bin(0.75), 3);
        assert_eq!(bin(0.1), 0, "0.1 px is nearest to 0");
        assert_eq!(bin(0.3), 1, "0.3 px is nearest to 0.25");
        assert_eq!(bin(0.6), 2, "0.6 px is nearest to 0.5");
        assert_eq!(bin(0.8), 3, "0.8 px is nearest to 0.75");
    }

    #[test]
    fn a_fraction_nearest_a_full_pixel_wraps_to_bin_zero() {
        assert_eq!(bin(0.9), 0);
        assert_eq!(bin(0.95), 0);
        assert_eq!(bin(3.9), 0);
    }

    #[test]
    fn the_subpixel_bin_depends_on_the_fraction_only() {
        for whole in [1.0, 2.0, 10.0, 123.0, 1000.0] {
            for fraction in [0.0, 0.25, 0.5, 0.75, 0.1, 0.6, 0.9] {
                assert_eq!(
                    bin(whole + fraction),
                    bin(fraction),
                    "{whole} + {fraction} lands in another bin than {fraction}"
                );
            }
        }
    }

    #[test]
    fn a_pen_left_of_the_origin_bins_like_one_a_whole_pixel_right() {
        assert_eq!(bin(-0.75), 1, "-0.75 px is 0.25 px past -1");
        assert_eq!(bin(-0.5), 2);
        assert_eq!(bin(-0.25), 3);
        assert_eq!(bin(-1.0), 0);
        assert_eq!(
            bin(-0.1),
            0,
            "-0.1 px is 0.9 px past -1, nearest a full pixel"
        );
        assert_eq!(bin(-10.75), bin(0.25));
    }

    #[test]
    fn every_subpixel_bin_is_below_four() {
        for step in 0..400 {
            let x = step as f32 * 0.037 - 7.0;
            assert!(bin(x) <= 3, "x = {x} gave bin {}", bin(x));
        }
    }

    /// A key's subpixel offset is its bin in quarter pixels — what a
    /// rasteriser shifts the glyph by.
    #[test]
    fn a_key_s_subpixel_offset_is_its_bin_in_quarter_pixels() {
        let font = AssetUUID::default();
        for (x, offset) in [
            (3.0, 0.0),
            (3.25, 0.25),
            (3.5, 0.5),
            (3.75, 0.75),
            (3.95, 0.0),
        ] {
            assert_eq!(
                GlyphKey::new(font, 7, 16.0, x).subpixel_offset(),
                offset,
                "x = {x}"
            );
        }
    }
}
