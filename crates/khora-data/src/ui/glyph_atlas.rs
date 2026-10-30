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

//! `GlyphAtlas` — the rasterised glyphs of every text, on the GPU.
//!
//! A runtime resource, like [`UiImageAtlas`](super::UiImageAtlas): two
//! [`TextureAtlas`]es — an R8 one for coverage masks, an RGBA one for colour
//! glyphs — each made the first time a glyph of its kind needs it, and the
//! slot of every glyph placed in them. The `glyphs_on_gpu` DataSystem fills
//! it before any agent runs; the UI lane only reads it.
//!
//! Not a `Resident<R>`: a glyph is not an asset.

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, MutexGuard};

use khora_core::asset::AssetUUID;
use khora_core::renderer::api::resource::TextureFormat;
use khora_core::renderer::GraphicsDevice;

use khora_core::renderer::api::resource::TextureViewId;
use khora_core::renderer::api::util::{AtlasRect, TextureAtlas};
use khora_core::ui::text::{GlyphImage, GlyphKey, GlyphKind};

/// Where one glyph sits in its atlas, and how it is placed against its pen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphSlot {
    /// The atlas it is in: [`GlyphKind::Mask`] the R8 one, [`GlyphKind::Color`]
    /// the RGBA one.
    pub kind: GlyphKind,
    /// Its rect in that atlas, in UV.
    pub rect: AtlasRect,
    /// The image's width, in pixels.
    pub width: u32,
    /// The image's height, in pixels.
    pub height: u32,
    /// The image's left edge, in pixels right of the pen.
    pub left: i32,
    /// The image's top edge, in pixels above the baseline.
    pub top: i32,
}

/// The glyph atlases and the slot of every glyph placed in them.
///
/// # Locks
///
/// One `Mutex` over the two atlases and the slots, above the device's leaf
/// lock in the one order RULES §5 allows: held to look up, insert or remove,
/// and across the device calls that upload into or replace its own textures
/// — never across a call to the rasteriser, nor anything that calls back.
pub struct GlyphAtlas {
    /// The side of each atlas, in pixels.
    size: u32,
    /// The atlases and the slots, behind the one leaf lock.
    state: Mutex<GlyphAtlasState>,
}

/// What [`GlyphAtlas`]'s lock guards.
struct GlyphAtlasState {
    /// The R8 atlas of coverage masks, once a mask needed it.
    mask: Option<TextureAtlas>,
    /// The RGBA atlas of colour glyphs, once a colour glyph needed it.
    color: Option<TextureAtlas>,
    /// Every glyph rasterised: its slot, or `None` for a glyph that draws
    /// nothing (a space), so it is not rasterised again.
    slots: HashMap<GlyphKey, Option<GlyphSlot>>,
    /// The glyphs in use a full atlas left out, with their kind, and the
    /// glyphs in use when they were: tried again once one of those is no
    /// longer in use — only a glyph going away makes room.
    left_out: Option<LeftOut>,
}

/// The glyphs a full atlas left out, and the glyphs in use then.
#[derive(Debug, Clone, Default)]
pub(crate) struct LeftOut {
    /// The glyphs left out, each with the atlas it needs.
    pub(crate) glyphs: HashMap<GlyphKey, GlyphKind>,
    /// Every glyph in use when they were left out.
    pub(crate) in_use: HashSet<GlyphKey>,
}

/// What placing a glyph came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Placement {
    /// It is in its atlas.
    Placed,
    /// Its atlas has no room left: a rebuild may make some.
    Full,
    /// It is larger than an empty atlas: no rebuild ever makes room. It is
    /// noted as drawn by nothing.
    NeverFits,
}

impl GlyphAtlas {
    /// An empty atlas whose two textures, when made, are `size` pixels square.
    #[must_use]
    pub fn new(size: u32) -> Self {
        Self {
            size,
            state: Mutex::new(GlyphAtlasState {
                mask: None,
                color: None,
                slots: HashMap::new(),
                left_out: None,
            }),
        }
    }

    fn state(&self) -> MutexGuard<'_, GlyphAtlasState> {
        // A panic while placing leaves the atlases and slots usable: at worst
        // a glyph is placed again.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The slot of the glyph `key` names, if it is placed in an atlas. `None`
    /// for a glyph not rasterised yet and for one that draws nothing.
    pub fn slot(&self, key: &GlyphKey) -> Option<GlyphSlot> {
        self.state().slots.get(key).copied().flatten()
    }

    /// The views of the two atlases, `(mask, colour)`; `None` for an atlas not
    /// made yet.
    pub fn views(&self) -> (Option<TextureViewId>, Option<TextureViewId>) {
        let state = self.state();
        (
            state.mask.as_ref().map(TextureAtlas::view),
            state.color.as_ref().map(TextureAtlas::view),
        )
    }

    /// The number of glyphs placed in an atlas (a glyph that draws nothing
    /// is not one).
    pub fn len(&self) -> usize {
        self.state()
            .slots
            .values()
            .filter(|slot| slot.is_some())
            .count()
    }

    /// Whether no glyph is placed in an atlas.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether the glyph `key` names was rasterised: placed, or known to
    /// draw nothing.
    pub(crate) fn knows(&self, key: &GlyphKey) -> bool {
        self.state().slots.contains_key(key)
    }

    /// Remembers that the glyph `key` names draws nothing (a space).
    pub(crate) fn note_nothing(&self, key: GlyphKey) {
        self.state().slots.insert(key, None);
    }

    /// Forgets every glyph of `font` — its file changed, so they are made
    /// again. Their room in the atlas is not given back: a shelf packer
    /// cannot free; a full atlas is rebuilt instead.
    pub(crate) fn forget_font(&self, font: AssetUUID) {
        self.state().slots.retain(|key, _| key.font != font);
    }

    /// The slot of the glyph `key` names, as far as the atlas knows it:
    /// `None` not rasterised, `Some(None)` drawn by nothing, `Some(Some(_))`
    /// placed.
    pub(crate) fn known(&self, key: &GlyphKey) -> Option<Option<GlyphSlot>> {
        self.state().slots.get(key).copied()
    }

    /// Places `image` as the glyph `key` names in the atlas of its kind,
    /// making that atlas if it is not yet.
    pub(crate) fn place(
        &self,
        device: &dyn GraphicsDevice,
        key: GlyphKey,
        image: &GlyphImage,
    ) -> Placement {
        let size = self.size;
        let mut state = self.state();
        let Some(atlas) = state.atlas_mut(device, size, image.kind) else {
            return Placement::Full;
        };
        if !atlas.could_hold(image.width, image.height) {
            state.slots.insert(key, None);
            return Placement::NeverFits;
        }
        let Some(rect) = atlas.allocate_and_upload(
            device,
            image.width,
            image.height,
            &image.pixels,
            bytes_per_pixel(image.kind),
        ) else {
            return Placement::Full;
        };
        state.slots.insert(key, Some(slot_of(image, rect)));
        Placement::Placed
    }

    /// The glyphs a full atlas left out, and the glyphs in use then.
    pub(crate) fn left_out(&self) -> Option<LeftOut> {
        self.state().left_out.clone()
    }

    /// Remembers the glyphs a full atlas left out, with the glyphs in use
    /// now; none left out forgets them.
    pub(crate) fn set_left_out(&self, left_out: LeftOut) {
        self.state().left_out = (!left_out.glyphs.is_empty()).then_some(left_out);
    }

    /// Builds the atlas of `kind` again with `images` alone — the glyphs of
    /// that kind the current layouts use — and destroys the old texture and
    /// its view. Every slot of the old atlas goes. Returns the images left
    /// out: those a fresh atlas has no room for once the others are placed (a
    /// glyph larger than an empty atlas is noted as drawn by nothing).
    pub(crate) fn rebuild(
        &self,
        device: &dyn GraphicsDevice,
        kind: GlyphKind,
        images: &[(GlyphKey, GlyphImage)],
    ) -> Vec<GlyphKey> {
        let size = self.size;
        let mut state = self.state();
        let old = match kind {
            GlyphKind::Mask => state.mask.take(),
            GlyphKind::Color => state.color.take(),
        };
        if let Some(old) = old {
            if let Err(error) = device.destroy_texture_view(old.view()) {
                log::warn!("glyph atlas: destroying the old view failed: {error}");
            }
            if let Err(error) = device.destroy_texture(old.texture()) {
                log::warn!("glyph atlas: destroying the old texture failed: {error}");
            }
        }
        state
            .slots
            .retain(|_, slot| slot.is_none_or(|slot| slot.kind != kind));
        let mut left_out = Vec::new();
        for (key, image) in images.iter().filter(|(_, image)| image.kind == kind) {
            let Some(atlas) = state.atlas_mut(device, size, kind) else {
                left_out.push(*key);
                continue;
            };
            if !atlas.could_hold(image.width, image.height) {
                state.slots.insert(*key, None);
                continue;
            }
            match atlas.allocate_and_upload(
                device,
                image.width,
                image.height,
                &image.pixels,
                bytes_per_pixel(kind),
            ) {
                Some(rect) => {
                    state.slots.insert(*key, Some(slot_of(image, rect)));
                }
                // No room left for this one; a smaller one may still fit.
                None => left_out.push(*key),
            }
        }
        left_out
    }
}

impl GlyphAtlasState {
    /// The atlas of `kind`, made on first need.
    fn atlas_mut(
        &mut self,
        device: &dyn GraphicsDevice,
        size: u32,
        kind: GlyphKind,
    ) -> Option<&mut TextureAtlas> {
        let (atlas, format, label) = match kind {
            GlyphKind::Mask => (&mut self.mask, TextureFormat::R8Unorm, "glyph mask atlas"),
            GlyphKind::Color => (
                &mut self.color,
                TextureFormat::Rgba8Unorm,
                "glyph colour atlas",
            ),
        };
        if atlas.is_none() {
            match TextureAtlas::new(device, size, format, label) {
                Ok(made) => *atlas = Some(made),
                Err(error) => {
                    log::warn!("glyph atlas: {label} cannot be made: {error}");
                    return None;
                }
            }
        }
        atlas.as_mut()
    }
}

/// Bytes per pixel of an image of `kind`.
fn bytes_per_pixel(kind: GlyphKind) -> u32 {
    match kind {
        GlyphKind::Mask => 1,
        GlyphKind::Color => 4,
    }
}

/// The slot of `image` placed at `rect`.
fn slot_of(image: &GlyphImage, rect: AtlasRect) -> GlyphSlot {
    GlyphSlot {
        kind: image.kind,
        rect,
        width: image.width,
        height: image.height,
        left: image.left,
        top: image.top,
    }
}

impl Default for GlyphAtlas {
    /// An empty atlas of two 1024 px square textures.
    fn default() -> Self {
        Self::new(1024)
    }
}
