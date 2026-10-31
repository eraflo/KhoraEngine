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

//! The frame's texts as glyph instances, and the bind group over the glyph
//! atlases they sample.
//!
//! A glyph is looked up by the key `glyphs_on_gpu` placed it under — its
//! font, id and size, and the subpixel bin of its shaped x — and drawn at
//! whole pixels: the text's origin rounded, the pen the whole pixel its
//! key's bin is counted from ([`GlyphKey::whole_pixel`]; the bin's fraction
//! is already in the image), the baseline its shaped y rounded. The image then
//! sits `left` right of the pen and `top` above the baseline, as large as it
//! is, so each of its texels lands on one pixel.

use khora_core::math::Vec2;
use khora_core::renderer::api::command::{
    BindGroupDescriptor, BindGroupEntry, BindGroupId, BindingResource,
};
use khora_core::renderer::api::resource::TextureViewId;
use khora_core::renderer::error::ResourceError;
use khora_core::renderer::GraphicsDevice;
use khora_core::ui::text::{GlyphKey, GlyphKind, ShapedGlyph};
use khora_data::ui::{ExtractedUiText, GlyphAtlas, GlyphSlot, UiScene};

use super::{fallback_view, UiGpu, UiInstanceData};

/// Glyphs the glyph storage holds before it first grows.
pub(super) const GLYPH_CAPACITY: usize = 1024;

/// One instance per glyph of the scene's texts that has a slot, text after
/// text in the scene's order, each text's glyphs in their shaped order. A
/// glyph not resident yet, or one that draws nothing, has none.
pub(super) fn glyph_instances_of(scene: &UiScene, atlas: &GlyphAtlas) -> Vec<UiInstanceData> {
    let mut instances = Vec::new();
    for text in &scene.texts {
        let origin = Vec2::new(text.pos.x.round(), text.pos.y.round());
        for glyph in text.glyphs.iter() {
            if let Some(slot) = atlas.slot(&key_of(text, glyph)) {
                instances.push(instance_of(text, glyph, origin, &slot));
            }
        }
    }
    instances
}

/// The key `glyphs_on_gpu` placed `glyph` of `text` under.
fn key_of(text: &ExtractedUiText, glyph: &ShapedGlyph) -> GlyphKey {
    GlyphKey::new(text.font, glyph.id, text.size_px, glyph.x)
}

/// The instance of `glyph`, of `text` at the whole-pixel `origin`, whose
/// image is in `slot`.
fn instance_of(
    text: &ExtractedUiText,
    glyph: &ShapedGlyph,
    origin: Vec2,
    slot: &GlyphSlot,
) -> UiInstanceData {
    let pen = origin.x + GlyphKey::whole_pixel(glyph.x);
    let baseline = origin.y + glyph.y.round();
    let color_glyph = if slot.kind == GlyphKind::Color {
        1.0
    } else {
        0.0
    };
    UiInstanceData {
        pos: [pen + slot.left as f32, baseline - slot.top as f32],
        size: [slot.width as f32, slot.height as f32],
        color: text.color.into(),
        params: [color_glyph, 0.0, 0.0, 0.0],
        uv_min: slot.rect.min.into(),
        uv_max: slot.rect.max.into(),
    }
}

/// The set-2 bind group of the glyph pipeline: the mask atlas's view, the
/// colour atlas's, and the sampler — the lane's one-texel fallback for an
/// atlas not made yet. It is kept while the two views it binds stay the
/// same (an atlas rebuilt is a new view), and the one it replaces is given
/// back.
pub(super) fn glyph_bind_group(
    built: &mut UiGpu,
    device: &dyn GraphicsDevice,
    atlas: &GlyphAtlas,
) -> Result<BindGroupId, ResourceError> {
    let (mask, colour) = atlas.views();
    let mask = match mask {
        Some(view) => view,
        None => fallback_view(built, device)?,
    };
    let colour = match colour {
        Some(view) => view,
        None => fallback_view(built, device)?,
    };
    if let Some((bound, group)) = built.glyph_bind_group {
        if bound == (mask, colour) {
            return Ok(group);
        }
    }
    let texture = |binding, view: TextureViewId| BindGroupEntry {
        binding,
        resource: BindingResource::TextureView(view),
        _phantom: std::marker::PhantomData,
    };
    let group = device.create_bind_group(&BindGroupDescriptor {
        label: Some("ui_glyph_bind_group"),
        layout: built.glyph_atlas_layout,
        entries: &[
            texture(0, mask),
            texture(1, colour),
            BindGroupEntry {
                binding: 2,
                resource: BindingResource::Sampler(built.sampler),
                _phantom: std::marker::PhantomData,
            },
        ],
    })?;
    if let Some((_, stale)) = built.glyph_bind_group.replace(((mask, colour), group)) {
        if let Err(e) = device.destroy_bind_group(stale) {
            log::warn!("UiLane: could not destroy the previous glyph bind group: {e:?}");
        }
    }
    Ok(group)
}
