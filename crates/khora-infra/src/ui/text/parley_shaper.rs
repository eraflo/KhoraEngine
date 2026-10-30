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

//! [`TextShaper`] through Parley.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use khora_core::asset::font::Font;
use khora_core::asset::AssetUUID;
use khora_core::math::Vec2;
use khora_core::ui::text::{
    FontError, FontFamily, ShapeError, ShapeRequest, ShapedGlyph, ShapedText, TextShaper,
};
use parley::{FontContext, LayoutContext, PositionedLayoutItem, StyleProperty};

/// Shapes UI text with Parley. Built with `Font::BUILT_IN` registered with
/// [`DEFAULT_FONT`](super::DEFAULT_FONT).
///
/// One leaf lock around Parley's contexts: held for one registration or one
/// shaping, never across a call out of this type.
pub struct ParleyShaper {
    state: Mutex<State>,
}

/// Parley's contexts. Each font gets a font context of its own, holding it
/// alone: two fonts of one family — the same file under two ids, or a font
/// and its reload — can never be mistaken for each other.
struct State {
    fonts: HashMap<AssetUUID, Registered>,
    layout: LayoutContext<()>,
}

/// One registered font: its own font context and the family it declares.
struct Registered {
    context: FontContext,
    family: String,
}

impl ParleyShaper {
    /// A shaper with `Font::BUILT_IN` already registered.
    pub fn new() -> Self {
        let shaper = Self {
            state: Mutex::new(State {
                fonts: HashMap::new(),
                layout: LayoutContext::new(),
            }),
        };
        // The embedded font is a valid font file: its registration cannot be
        // refused.
        let built_in = shaper.register_font(Font::BUILT_IN, Arc::from(super::DEFAULT_FONT));
        debug_assert!(built_in.is_ok(), "the default font is readable");
        shaper
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        // A panic while shaping leaves the contexts as Parley left them,
        // still usable.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Default for ParleyShaper {
    fn default() -> Self {
        Self::new()
    }
}

impl TextShaper for ParleyShaper {
    fn register_font(&self, id: AssetUUID, data: Arc<[u8]>) -> Result<FontFamily, FontError> {
        // Parsed outside the lock: only the insertion is under it.
        let mut context = FontContext::new();
        let family = super::register_first_face(&mut context.collection, data)?;
        self.state().fonts.insert(
            id,
            Registered {
                context,
                family: family.clone(),
            },
        );
        Ok(FontFamily { name: family })
    }

    fn shape(&self, request: &ShapeRequest<'_>) -> Result<ShapedText, ShapeError> {
        let text = drawable_text(request.text);
        // A width that is not a number is no limit (Parley asserts on a NaN
        // line width); a negative or zero one breaks at every opportunity.
        let max_width = request.max_width.filter(|width| !width.is_nan());

        let mut state = self.state();
        let State { fonts, layout } = &mut *state;
        let font = fonts
            .get_mut(&request.font)
            .ok_or(ShapeError::UnknownFont(request.font))?;
        // A size that is not a positive, finite number of pixels draws
        // nothing — bad data renders nothing, it never panics nor sprawls.
        if !(request.size_px.is_finite() && request.size_px > 0.0) {
            return Ok(ShapedText {
                glyphs: Vec::new(),
                size: Vec2::ZERO,
                lines: 0,
            });
        }

        let mut builder = layout.ranged_builder(&mut font.context, &text, 1.0, true);
        builder.push_default(StyleProperty::FontFamily(parley::FontFamily::named(
            &font.family,
        )));
        builder.push_default(StyleProperty::FontSize(request.size_px));
        let mut text = builder.build(&text);
        text.break_all_lines(max_width);

        let mut glyphs = Vec::new();
        let mut lines = 0;
        // The widest line, each measured without the whitespace hanging past
        // a soft break (Parley counts a trailing space of the last line); a
        // line holding no glyph (an empty text) is no wider than nothing,
        // whatever advance Parley gives its empty run.
        let mut width = 0.0_f32;
        for line in text.lines() {
            lines += 1;
            let before = glyphs.len();
            for item in line.items() {
                if let PositionedLayoutItem::GlyphRun(run) = item {
                    glyphs.extend(run.positioned_glyphs().filter_map(|glyph| {
                        Some(ShapedGlyph {
                            id: u16::try_from(glyph.id).ok()?,
                            x: glyph.x,
                            y: glyph.y,
                        })
                    }));
                }
            }
            if glyphs.len() > before {
                let metrics = line.metrics();
                width = width.max(metrics.advance - metrics.hanging_advance);
            }
        }
        Ok(ShapedText {
            glyphs,
            size: Vec2::new(width, text.height()),
            lines,
        })
    }
}

/// The text as it is drawn: a tab is whitespace four spaces wide (fonts have
/// no glyph for it, so it would come back as the box of `.notdef`), and the
/// other control characters but line breaks are not drawn at all.
fn drawable_text(text: &str) -> std::borrow::Cow<'_, str> {
    let drawn = |c: char| !c.is_control() || c == '\n' || c == '\r';
    if text.chars().all(drawn) {
        return std::borrow::Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        match c {
            '\t' => out.push_str("    "),
            c if drawn(c) => out.push(c),
            _ => {}
        }
    }
    std::borrow::Cow::Owned(out)
}
