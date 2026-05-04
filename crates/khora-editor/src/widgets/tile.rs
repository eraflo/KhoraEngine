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

//! Asset-browser tile widget — type-coloured gradient thumbnails.

use khora_sdk::editor_ui::{EditorTheme, FontFamilyHint, Icon, TextAlign, UiBuilder};

use super::paint::{paint_icon, paint_text_size, with_alpha};

/// Visual category for an asset tile. Drives the gradient + icon + format
/// glyph. Variants are limited to types the editor's asset browser
/// actually knows how to surface — adding a new asset type is a single
/// `match` arm in `AssetBrowserPanel::classify_extension` plus a variant
/// here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetTileKind {
    Mesh,
    Texture,
    Audio,
    Shader,
    Scene,
    Unknown,
}

impl AssetTileKind {
    fn icon(self) -> Icon {
        match self {
            Self::Mesh => Icon::Cube,
            Self::Texture => Icon::Image,
            Self::Audio => Icon::Music,
            Self::Shader => Icon::Zap,
            Self::Scene => Icon::Globe,
            Self::Unknown => Icon::Box,
        }
    }

    /// Returns the top accent colour for the tile gradient (the bottom
    /// always falls into `theme.surface` so the tile blends with the panel
    /// background regardless of category).
    fn accent(self, theme: &EditorTheme) -> [f32; 4] {
        let raw = match self {
            Self::Mesh => theme.accent_a,
            Self::Texture => theme.accent_c,
            Self::Audio => theme.success,
            Self::Shader => theme.accent_a,
            Self::Scene => theme.accent_a,
            Self::Unknown => theme.surface_active,
        };
        with_alpha(raw, 0.85)
    }

    fn glyph_label(self) -> &'static str {
        match self {
            Self::Mesh => "FBX",
            Self::Texture => "TEX",
            Self::Audio => "OGG",
            Self::Shader => "GLSL",
            Self::Scene => "SCN",
            Self::Unknown => "—",
        }
    }
}

/// Paints an asset tile (thumbnail + name) and reports interaction.
///
/// `origin` is the top-left in screen-space, `size` is the total tile box
/// (thumbnail + name strip below). The tile self-measures so the caller only
/// has to grid-place identical-size boxes.
#[allow(clippy::too_many_arguments)] // UI paint helper — splitting hurts readability.
pub fn paint_asset_tile(
    ui: &mut dyn UiBuilder,
    id_salt: &str,
    origin: [f32; 2],
    size: [f32; 2],
    name: &str,
    kind: AssetTileKind,
    selected: bool,
    theme: &EditorTheme,
) -> bool {
    let [w, h] = size;
    let thumb_h = w; // square
    let name_y = origin[1] + thumb_h + 4.0;

    // Outer hit area (used for hover background)
    let outer = [origin[0], origin[1], w, h];
    let interaction = ui.interact_rect(id_salt, outer);

    if selected {
        ui.paint_rect_filled(origin, [w, h], with_alpha(theme.primary, 0.18), 6.0);
    } else if interaction.hovered {
        ui.paint_rect_filled(origin, [w, h], with_alpha(theme.surface_elevated, 0.4), 6.0);
    }

    // Thumbnail
    let thumb_x = origin[0] + 4.0;
    let thumb_y = origin[1] + 4.0;
    let tw = w - 8.0;
    let th = thumb_h - 8.0;
    let accent = kind.accent(theme);
    ui.paint_rect_filled([thumb_x, thumb_y], [tw, th], accent, 4.0);
    ui.paint_rect_stroke(
        [thumb_x, thumb_y],
        [tw, th],
        with_alpha(theme.separator, 0.55),
        4.0,
        1.0,
    );
    if selected {
        ui.paint_rect_stroke([thumb_x, thumb_y], [tw, th], theme.primary, 4.0, 1.5);
    }

    // Centered icon
    let icon_size = (tw * 0.45).clamp(20.0, 36.0);
    paint_icon(
        ui,
        [
            thumb_x + (tw - icon_size) * 0.5,
            thumb_y + (th - icon_size) * 0.5,
        ],
        kind.icon(),
        icon_size,
        with_alpha(theme.text, 0.85),
    );

    // Format glyph bottom-right
    ui.paint_text_styled(
        [thumb_x + tw - 4.0, thumb_y + th - 14.0],
        kind.glyph_label(),
        9.5,
        with_alpha([1.0, 1.0, 1.0, 1.0], 0.55),
        FontFamilyHint::Monospace,
        TextAlign::Right,
    );

    // Name (clipped if too long — egui handles eg_text overflow per-character)
    let truncated = if name.chars().count() > 14 {
        let mut s: String = name.chars().take(13).collect();
        s.push('…');
        s
    } else {
        name.to_owned()
    };
    paint_text_size(
        ui,
        [origin[0] + 4.0, name_y],
        &truncated,
        11.0,
        if selected { theme.text } else { theme.text_dim },
    );

    interaction.clicked
}
