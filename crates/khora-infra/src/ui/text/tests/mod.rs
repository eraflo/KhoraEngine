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

//! The UI text backends on the real default font: `ParleyShaper` shapes,
//! `SwashRasterizer` rasterises, both register fonts, and both hold up when
//! many threads call them at once.
//!
//! The glyph ids below are read from the `cmap` of `InterVariable.ttf`
//! (Inter 4.1, SHA-256 in `assets/fonts/README.md`); a new font file means new
//! ids.

mod concurrency;
mod font_registration;
mod glyph_keys;
mod rasterising;
mod shaping;
mod trait_objects;

use std::sync::Arc;

use khora_core::asset::font::Font;
use khora_core::asset::AssetUUID;
use khora_core::ui::text::{GlyphKey, ShapeRequest, ShapedText, TextShaper};

use super::{ParleyShaper, DEFAULT_FONT};

/// Inter's glyph for `H`.
const H: u16 = 161;
/// Inter's glyph for `a`.
const A: u16 = 507;
/// Inter's glyph for `b`.
const B: u16 = 578;
/// Inter's glyph for `i`.
const I: u16 = 689;
/// Inter's glyph for `p`.
const P: u16 = 843;
/// Inter's glyph for the space.
const SPACE: u16 = 1777;
/// How many glyphs Inter has: every id from this one up names none.
const INTER_GLYPH_COUNT: u16 = 2937;

/// A second, monospaced font (IBM Plex Mono, the tools' monospace face), only
/// to see a registration replace a font with one that shapes differently.
const PLEX_MONO: &[u8] =
    include_bytes!("../../../../../khora-tool-ui/assets/fonts/IBMPlexMono-Regular.ttf");

/// The engine's default font, as the backends take it.
fn default_font() -> Arc<[u8]> {
    Arc::from(DEFAULT_FONT)
}

/// The monospaced font, as the backends take it.
fn plex_mono() -> Arc<[u8]> {
    Arc::from(PLEX_MONO)
}

/// An id no backend has registered.
fn fresh_id() -> AssetUUID {
    AssetUUID::new()
}

/// The id of the font every backend is built with.
fn built_in() -> AssetUUID {
    Font::BUILT_IN
}

/// A request for `text` in `font` at `size_px`, without a width limit.
fn request(text: &str, font: AssetUUID, size_px: f32) -> ShapeRequest<'_> {
    ShapeRequest {
        text,
        font,
        size_px,
        max_width: None,
    }
}

/// `text` shaped in the built-in font at `size_px`; panics if refused.
fn shaped(shaper: &ParleyShaper, text: &str, size_px: f32) -> ShapedText {
    shaper
        .shape(&request(text, built_in(), size_px))
        .unwrap_or_else(|e| panic!("{text:?} at {size_px} px was refused: {e}"))
}

/// The key of `glyph` of `font` at `size_px` (a whole number of quarter
/// pixels) in subpixel bin `bin`, built field by field so the rasteriser's
/// tests do not go through `GlyphKey::new`.
fn key(font: AssetUUID, glyph: u16, size_px: f32, bin: u8) -> GlyphKey {
    GlyphKey {
        font,
        glyph,
        size_q: (size_px * 4.0) as u32,
        subpixel_x: bin,
    }
}
