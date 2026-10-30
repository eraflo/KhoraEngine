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

//! Every glyph of every text, rasterised once and placed in the GPU atlas.
//!
//! The `glyphs_on_gpu` DataSystem makes each glyph of each
//! [`UiTextLayout`](crate::ui::UiTextLayout) resident in the runtime's
//! [`GlyphAtlas`](crate::ui::GlyphAtlas), rasterising it through the runtime's
//! `GlyphRasterizer`, before any agent runs. A full atlas is built again from
//! the glyphs the current layouts use; the old texture is destroyed.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use khora_core::asset::font::Font;
use khora_core::ecs::entity::EntityId;
use khora_core::lane::OutputDeck;
use khora_core::renderer::GraphicsDevice;
use khora_core::ui::text::{GlyphImage, GlyphKey, GlyphKind, GlyphRasterizer};
use khora_core::Runtime;

use super::text_layout::new_fonts;
use crate::ecs::{
    Changed, ComponentKey, DataSystemRegistration, HandleComponent, SystemContext, TickPhase, World,
};
use crate::ui::glyph_atlas::{LeftOut, Placement};
use crate::ui::{GlyphAtlas, UiTextLayout};

/// Registers with the rasteriser the fonts new this run, and makes every
/// glyph of the layouts that changed since `ctx.since` resident in the glyph
/// atlas — every layout's when an atlas was built again.
pub(crate) fn glyphs_on_gpu(
    world: &mut World,
    runtime: &Runtime,
    _deck: &mut OutputDeck,
    ctx: SystemContext,
) {
    let (Some(device), Some(rasterizer), Some(atlas)) = (
        runtime.backends.get::<Arc<dyn GraphicsDevice>>(),
        runtime.backends.get::<Arc<dyn GlyphRasterizer>>(),
        runtime.resources.get::<Arc<GlyphAtlas>>(),
    ) else {
        return;
    };

    // 1. A font whose file is new: registered, and its glyphs made again.
    let mut new = HashSet::new();
    for (font, handle) in new_fonts(world, ctx.since) {
        if let Err(error) = rasterizer.register_font(font, Arc::from(&handle.data[..])) {
            log::warn!("glyphs_on_gpu: font {font} cannot be read: {error}");
        }
        atlas.forget_font(font);
        new.insert(font);
    }

    // Every glyph rasterised at most once a run, whatever path asks for it.
    let mut images = Images {
        rasterizer: rasterizer.as_ref(),
        made: HashMap::new(),
    };

    // 2. The glyphs a full atlas left out are tried again once a glyph that
    //    was in use is no longer — only a glyph going away makes room.
    let mut rebuilt: Vec<GlyphKind> = Vec::new();
    let mut left_out: HashMap<GlyphKey, GlyphKind> = HashMap::new();
    let mut in_use: Option<Vec<GlyphKey>> = None;
    if let Some(before) = atlas.left_out() {
        let now = keys(world, |_, _| true);
        let now_set: HashSet<GlyphKey> = now.iter().copied().collect();
        if before.in_use.iter().any(|key| !now_set.contains(key)) {
            let mut kinds: Vec<GlyphKind> = before.glyphs.values().copied().collect();
            kinds.sort_by_key(|kind| *kind == GlyphKind::Color);
            kinds.dedup();
            for kind in kinds {
                rebuilt.push(kind);
                left_out.extend(rebuild(atlas, device.as_ref(), &mut images, &now, kind));
            }
        } else {
            left_out = before.glyphs;
        }
        in_use = Some(now);
    }

    // 3. The glyphs of the layouts that changed, or whose font is new, each
    //    rasterised once; a full atlas is built again from every glyph in use,
    //    once a run per kind.
    let changed: HashSet<EntityId> = world
        .query::<(EntityId, Changed<UiTextLayout>)>()
        .since(ctx.since)
        .map(|(entity, _)| entity)
        .collect();
    let wanted = keys(world, |entity, layout| {
        changed.contains(&entity) || new.contains(&layout.font)
    });
    for key in wanted {
        if atlas.knows(&key) || left_out.contains_key(&key) {
            continue;
        }
        // Rasterised outside the atlas's lock.
        let Some(image) = images.get(&key) else {
            atlas.note_nothing(key);
            continue;
        };
        let kind = image.kind;
        match atlas.place(device.as_ref(), key, image) {
            Placement::Placed => {}
            Placement::NeverFits => {
                log::warn!(
                    "glyphs_on_gpu: glyph {} of font {} at {} px is larger than the glyph \
                     atlas: not drawn",
                    key.glyph,
                    key.font,
                    key.size_px()
                );
            }
            Placement::Full if rebuilt.contains(&kind) => {
                left_out.insert(key, kind);
            }
            Placement::Full => {
                rebuilt.push(kind);
                let now = in_use.get_or_insert_with(|| keys(world, |_, _| true));
                left_out.extend(rebuild(atlas, device.as_ref(), &mut images, now, kind));
            }
        }
    }

    // 4. What was left out is remembered with the glyphs in use now.
    let in_use = if left_out.is_empty() {
        HashSet::new()
    } else {
        in_use
            .unwrap_or_else(|| keys(world, |_, _| true))
            .into_iter()
            .collect()
    };
    atlas.set_left_out(LeftOut {
        glyphs: left_out,
        in_use,
    });
}

/// The glyph images of one run, each rasterised at most once.
struct Images<'r> {
    rasterizer: &'r dyn GlyphRasterizer,
    made: HashMap<GlyphKey, Option<GlyphImage>>,
}

impl Images<'_> {
    /// The image of `key`, rasterised the first time it is asked for this
    /// run; `None` for a glyph that draws nothing.
    fn get(&mut self, key: &GlyphKey) -> Option<&GlyphImage> {
        let rasterizer = self.rasterizer;
        self.made
            .entry(*key)
            .or_insert_with(|| rasterizer.rasterize(key))
            .as_ref()
    }
}

/// Builds the atlas of `kind` again from the glyphs in use, rasterising only
/// those of that kind or not yet rasterised — never a glyph of the other
/// kind placed already, nor one known to draw nothing. Returns the glyphs
/// left out.
fn rebuild(
    atlas: &GlyphAtlas,
    device: &dyn GraphicsDevice,
    images: &mut Images<'_>,
    in_use: &[GlyphKey],
    kind: GlyphKind,
) -> Vec<(GlyphKey, GlyphKind)> {
    let mut wanted = Vec::new();
    for key in in_use {
        let keep = match atlas.known(key) {
            Some(Some(slot)) => slot.kind == kind,
            Some(None) => false,
            None => true,
        };
        if !keep {
            continue;
        }
        match images.get(key) {
            Some(image) if image.kind == kind => wanted.push((*key, image.clone())),
            Some(_) => {}
            None => atlas.note_nothing(*key),
        }
    }
    atlas
        .rebuild(device, kind, &wanted)
        .into_iter()
        .map(|key| (key, kind))
        .collect()
}

/// The keys of the glyphs of every layout `wanted` keeps, once each, in the
/// order the layouts come: each glyph at its layout's font and size, binned
/// by its x within the layout (a text's origin is snapped to whole pixels
/// when drawn).
fn keys(world: &World, wanted: impl Fn(EntityId, &UiTextLayout) -> bool) -> Vec<GlyphKey> {
    let mut seen = HashSet::new();
    let mut keys = Vec::new();
    for (entity, layout) in world.query::<(EntityId, &UiTextLayout)>() {
        if !wanted(entity, layout) {
            continue;
        }
        for glyph in &layout.glyphs {
            let key = GlyphKey::new(layout.font, glyph.id, layout.size_px, glyph.x);
            if seen.insert(key) {
                keys.push(key);
            }
        }
    }
    keys
}

inventory::submit! {
    DataSystemRegistration {
        name: "glyphs_on_gpu",
        phase: TickPhase::PreExtract,
        run: glyphs_on_gpu,
        order_hint: 0,
        runs_after: &["ui_text_layout"],
        observes: &[
            ComponentKey::of::<UiTextLayout>,
            ComponentKey::of::<HandleComponent<Font>>,
        ],
    }
}

#[cfg(test)]
mod tests;
