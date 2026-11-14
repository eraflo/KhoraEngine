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

pub use super::glyph_rasterizer::{GlyphImage, GlyphKey, GlyphKind, GlyphRasterizer};

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
