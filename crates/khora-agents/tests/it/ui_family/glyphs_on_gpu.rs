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

//! Texts shaped by Parley and rasterised by swash into the glyph atlas,
//! through the `PreExtract` phase the engine runs before the flows project
//! the UI.

use std::collections::HashSet;
use std::sync::Arc;

use khora_control::substrate::run_data_systems;
use khora_core::asset::font::Font;
use khora_core::lane::OutputDeck;
use khora_core::renderer::GraphicsDevice;
use khora_core::ui::text::{GlyphKey, GlyphKind, GlyphRasterizer, TextShaper};
use khora_core::Runtime;
use khora_data::ecs::{TickPhase, World};
use khora_data::ui::{GlyphAtlas, UiText, UiTextLayout, UiTransform};
use khora_infra::{ParleyShaper, SwashRasterizer};

use crate::shadows_family::fixtures::SubmitLog;

/// **"Hello" in the built-in font is four mask glyphs in the R8 atlas.** One
/// `PreExtract`, with Parley shaping, swash rasterising and a device that
/// makes whatever it is asked for, leaves every glyph of the text resident:
/// four glyphs — 'l' is used twice — each a coverage mask with a slot of its
/// own (one per subpixel bin the pens fall in), and no colour atlas.
#[test]
fn hello_in_the_built_in_font_is_four_mask_glyphs_in_the_r8_atlas() {
    let mut world = World::new();
    let entity = world.spawn((
        UiTransform::default(),
        UiText {
            content: "Hello".to_owned(),
            font: Font::BUILT_IN,
            size: 16.0,
            ..Default::default()
        },
    ));
    let atlas = Arc::new(GlyphAtlas::default());
    let mut runtime = Runtime::new();
    let shaper: Arc<dyn TextShaper> = Arc::new(ParleyShaper::new());
    runtime.backends.insert(shaper);
    let rasterizer: Arc<dyn GlyphRasterizer> = Arc::new(SwashRasterizer::new());
    runtime.backends.insert(rasterizer);
    let device: Arc<dyn GraphicsDevice> = Arc::new(SubmitLog::new());
    runtime.backends.insert(device);
    runtime.resources.insert(Arc::clone(&atlas));

    run_data_systems(
        &mut world,
        &runtime,
        &mut OutputDeck::new(),
        TickPhase::PreExtract,
    );

    let layout = world
        .get::<UiTextLayout>(entity)
        .expect("the text is laid out in PreExtract");
    assert_eq!(layout.glyphs.len(), 5, "{:?}", layout.glyphs);
    let ids: HashSet<u16> = layout.glyphs.iter().map(|glyph| glyph.id).collect();
    assert_eq!(ids.len(), 4, "H, e, l, o: {:?}", layout.glyphs);
    assert_eq!(
        layout.glyphs[2].id, layout.glyphs[3].id,
        "the two 'l's are one glyph"
    );

    let keys: HashSet<GlyphKey> = layout
        .glyphs
        .iter()
        .map(|glyph| GlyphKey::new(layout.font, glyph.id, layout.size_px, glyph.x))
        .collect();
    for key in &keys {
        let slot = atlas
            .slot(key)
            .unwrap_or_else(|| panic!("glyph {} is resident: {key:?}", key.glyph));
        assert_eq!(slot.kind, GlyphKind::Mask, "{key:?}");
        assert!(slot.width > 0 && slot.height > 0, "{key:?}: {slot:?}");
    }
    assert_eq!(atlas.len(), keys.len(), "one slot per key, nothing else");
    assert!(atlas.views().0.is_some(), "the R8 atlas is made");
    assert_eq!(atlas.views().1, None, "no colour glyph, no colour atlas");
}
