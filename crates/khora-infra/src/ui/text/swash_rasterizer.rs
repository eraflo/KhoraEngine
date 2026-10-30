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

//! [`GlyphRasterizer`] through swash.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use khora_core::asset::font::Font;
use khora_core::asset::AssetUUID;
use khora_core::ui::text::{FontError, GlyphImage, GlyphKey, GlyphKind, GlyphRasterizer};
use swash::scale::image::Content;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::zeno::{Format, Vector};
use swash::FontRef;

/// Rasterises UI glyphs with swash. Built with `Font::BUILT_IN` registered
/// with [`DEFAULT_FONT`](super::DEFAULT_FONT).
///
/// One leaf lock around the fonts and swash's scaling context: held for one
/// registration or one glyph, never across a call out of this type.
pub struct SwashRasterizer {
    state: Mutex<State>,
}

struct State {
    fonts: HashMap<AssetUUID, Registered>,
    scale: ScaleContext,
}

/// A registered font: its bytes, where the font starts in them, and how many
/// glyphs it has.
struct Registered {
    data: Arc<[u8]>,
    offset: u32,
    key: swash::CacheKey,
    glyph_count: u16,
}

impl Registered {
    fn font(&self) -> FontRef<'_> {
        FontRef {
            data: &self.data,
            offset: self.offset,
            key: self.key,
        }
    }
}

/// Where a glyph's image comes from, in order: a colour outline, a colour
/// bitmap, the plain outline.
const SOURCES: [Source; 3] = [
    Source::ColorOutline(0),
    Source::ColorBitmap(StrikeWith::BestFit),
    Source::Outline,
];

impl SwashRasterizer {
    /// A rasteriser with `Font::BUILT_IN` already registered.
    pub fn new() -> Self {
        let rasterizer = Self {
            state: Mutex::new(State {
                fonts: HashMap::new(),
                scale: ScaleContext::new(),
            }),
        };
        // The embedded font is a valid font file: its registration cannot be
        // refused.
        let built_in = rasterizer.register_font(Font::BUILT_IN, Arc::from(super::DEFAULT_FONT));
        debug_assert!(built_in.is_ok(), "the default font is readable");
        rasterizer
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        // A panic while rasterising leaves the fonts and the scaling context
        // usable.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Default for SwashRasterizer {
    fn default() -> Self {
        Self::new()
    }
}

impl GlyphRasterizer for SwashRasterizer {
    fn register_font(&self, id: AssetUUID, data: Arc<[u8]>) -> Result<(), FontError> {
        // Read outside the lock: only the insertion is under it. The file is
        // a font by the rule the shaper reads it by, and its first face is
        // the one drawn.
        super::register_first_face(
            &mut parley::fontique::Collection::new(Default::default()),
            Arc::clone(&data),
        )?;
        let font = FontRef::from_index(&data, 0).ok_or(FontError::Unreadable)?;
        let (offset, key) = (font.offset, font.key);
        let glyph_count = font.metrics(&[]).glyph_count;
        if glyph_count == 0 {
            return Err(FontError::Unreadable);
        }
        let registered = Registered {
            data: Arc::clone(&data),
            offset,
            key,
            glyph_count,
        };
        self.state().fonts.insert(id, registered);
        Ok(())
    }

    fn rasterize(&self, key: &GlyphKey) -> Option<GlyphImage> {
        let mut state = self.state();
        let State { fonts, scale } = &mut *state;
        let registered = fonts.get(&key.font)?;
        // A size of zero is "unscaled" to swash: it would draw the glyph in
        // font units, hundreds of pixels wide.
        if key.glyph >= registered.glyph_count || key.size_q == 0 {
            return None;
        }
        let mut scaler = scale
            .builder(registered.font())
            .size(key.size_px())
            .hint(false)
            .build();
        let image = Render::new(&SOURCES)
            .format(Format::Alpha)
            .offset(Vector::new(key.subpixel_offset(), 0.0))
            .render(&mut scaler, key.glyph)?;
        let (width, height) = (image.placement.width, image.placement.height);
        if width == 0 || height == 0 {
            return None;
        }
        let kind = match image.content {
            Content::Mask => GlyphKind::Mask,
            Content::Color | Content::SubpixelMask => GlyphKind::Color,
        };
        Some(GlyphImage {
            width,
            height,
            left: image.placement.left,
            top: image.placement.top,
            kind,
            pixels: image.data,
        })
    }
}
