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

use khora_sdk::editor_ui::{FontFamilyHint, Icon, TextAlign, UiBuilder, UiTheme};

use khora_tool_ui::widgets::paint;
use khora_tool_ui::widgets::with_alpha;

/// Visual category for an asset tile. Drives the gradient + icon + format
/// glyph. Variants map onto the canonical type names produced by
/// `khora_io::asset::IndexBuilder::asset_type_for_extension`. Add a new
/// variant when a new asset *category* lands engine-side; per-extension
/// distinctions belong in the IndexBuilder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetTileKind {
    Mesh,
    Texture,
    Audio,
    Shader,
    Scene,
    /// Authored materials (`.kmat`) — RON material definitions referenced
    /// by entities through `MaterialRef::Asset`.
    Material,
    /// Gameplay scripts (`.erg`, Ergon) — hot-reloadable: the watcher
    /// recompiles an edited script and the running behaviors carry on in
    /// the new code.
    Script,
    Unknown,
}

impl AssetTileKind {
    pub(crate) fn icon(self) -> Icon {
        match self {
            Self::Mesh => Icon::Cube,
            Self::Texture => Icon::Image,
            Self::Audio => Icon::Music,
            Self::Shader => Icon::Zap,
            Self::Scene => Icon::Globe,
            Self::Material => Icon::Circle,
            Self::Script => Icon::Code,
            Self::Unknown => Icon::Box,
        }
    }

    /// Returns the top accent colour for the tile gradient (the bottom
    /// always falls into `theme.surface` so the tile blends with the panel
    /// background regardless of category).
    pub(crate) fn accent(self, theme: &UiTheme) -> [f32; 4] {
        let raw = match self {
            Self::Mesh => theme.accent_a,
            Self::Texture => theme.accent_c,
            Self::Audio => theme.success,
            Self::Shader => theme.accent_a,
            Self::Scene => theme.accent_a,
            Self::Material => theme.accent_c,
            Self::Script => theme.warning,
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
            Self::Material => "MAT",
            Self::Script => "KSCR",
            Self::Unknown => "—",
        }
    }
}

/// What the user did to an asset tile this frame.
///
/// Tiles surface both single- and double-click distinctly: single click
/// selects (asset browser shows metadata), double-click is the "primary
/// action" (load a scene, open a script in OS-default editor, etc.).
#[derive(Debug, Default, Clone, Copy)]
pub struct AssetTileInteraction {
    pub clicked: bool,
    pub double_clicked: bool,
    /// Pointer is over the tile — kept here for callers that want to
    /// surface a tooltip or preview without re-running the rect
    /// interaction. Currently unused by the asset browser, but the
    /// future inspector preview will read it.
    #[allow(dead_code)]
    pub hovered: bool,
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
    badge: Option<crate::panels::problems::view::TileBadge>,
    theme: &UiTheme,
) -> AssetTileInteraction {
    let [w, h] = size;
    let thumb_h = w; // square
    let name_y = origin[1] + thumb_h + 4.0;

    // Outer hit area (used for hover background)
    let outer = [origin[0], origin[1], w, h];
    let interaction = ui.interact_rect(id_salt, outer);

    // Gold marks the selection, here as everywhere else in the editor.
    if selected {
        ui.paint_rect_filled(origin, [w, h], with_alpha(theme.accent_c, 0.12), 6.0);
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
        ui.paint_rect_stroke([thumb_x, thumb_y], [tw, th], theme.accent_c, 4.0, 1.5);
    }

    // Centered icon
    let icon_size = (tw * 0.45).clamp(20.0, 36.0);
    paint::icon(
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

    // A script with problems says so on its thumbnail: the severity's own
    // glyph and how many, on the severity's tint — never the colour alone.
    if let Some(badge) = badge {
        let (color, glyph) = match badge.severity {
            khora_sdk::khora_core::script::ProblemSeverity::Error => (theme.error, Icon::Error),
            khora_sdk::khora_core::script::ProblemSeverity::Warning => (theme.warning, Icon::Warn),
        };
        let count = badge.count.to_string();
        let digits = ui.measure_text(&count, 10.0, FontFamilyHint::Monospace)[0];
        let bw = 6.0 + 11.0 + 3.0 + digits + 6.0;
        let bh = 16.0;
        let bx = thumb_x + tw - bw - 4.0;
        let by = thumb_y + 4.0;
        ui.paint_rect_filled([bx, by], [bw, bh], theme.surface, 8.0);
        ui.paint_rect_filled([bx, by], [bw, bh], with_alpha(color, 0.22), 8.0);
        ui.paint_rect_stroke([bx, by], [bw, bh], with_alpha(color, 0.65), 8.0, 1.0);
        paint::icon(ui, [bx + 6.0, by + 2.5], glyph, 11.0, color);
        ui.paint_text_styled(
            [bx + 6.0 + 11.0 + 3.0, by + 2.5],
            &count,
            10.0,
            theme.text,
            FontFamilyHint::Monospace,
            TextAlign::Left,
        );
    }

    // Name (clipped if too long — egui handles eg_text overflow per-character)
    let truncated = if name.chars().count() > 14 {
        let mut s: String = name.chars().take(13).collect();
        s.push('…');
        s
    } else {
        name.to_owned()
    };
    paint::text(
        ui,
        [origin[0] + 4.0, name_y],
        &truncated,
        11.0,
        if selected { theme.text } else { theme.text_dim },
    );

    AssetTileInteraction {
        clicked: interaction.clicked,
        double_clicked: interaction.double_clicked,
        hovered: interaction.hovered,
    }
}

/// Paints the disclosure chip of a tile that holds parts (a glTF file's
/// scenes, meshes, materials, textures) and reports whether it was clicked.
///
/// A neutral pill on the thumbnail's bottom-left corner — the problems
/// badge holds the top-right, the format glyph the bottom-right: a chevron
/// (right while closed, down while open) and how many parts. Drawn after
/// the tile, so the chip takes the click and the tile keeps the rest.
pub fn paint_parts_chip(
    ui: &mut dyn UiBuilder,
    id_salt: &str,
    tile_origin: [f32; 2],
    tile_width: f32,
    count: usize,
    open: bool,
    theme: &UiTheme,
) -> bool {
    let thumb_x = tile_origin[0] + 4.0;
    let thumb_y = tile_origin[1] + 4.0;
    let thumb_h = tile_width - 8.0;
    let text = count.to_string();
    let digits = ui.measure_text(&text, 10.0, FontFamilyHint::Monospace)[0];
    let cw = 5.0 + 10.0 + 3.0 + digits + 6.0;
    let ch = 16.0;
    let cx = thumb_x + 4.0;
    let cy = thumb_y + thumb_h - ch - 4.0;
    let interaction = ui.interact_rect(id_salt, [cx, cy, cw, ch]);
    ui.tooltip_for_last(if open {
        "Hide its parts"
    } else {
        "Show its parts"
    });
    let stroke = if interaction.hovered {
        theme.text_dim
    } else {
        with_alpha(theme.separator, 0.65)
    };
    ui.paint_rect_filled([cx, cy], [cw, ch], theme.surface, 8.0);
    ui.paint_rect_stroke([cx, cy], [cw, ch], stroke, 8.0, 1.0);
    paint::icon(
        ui,
        [cx + 5.0, cy + 3.0],
        if open {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        },
        10.0,
        theme.text_dim,
    );
    ui.paint_text_styled(
        [cx + 5.0 + 10.0 + 3.0, cy + 2.5],
        &text,
        10.0,
        theme.text,
        FontFamilyHint::Monospace,
        TextAlign::Left,
    );
    interaction.clicked
}

/// Paints the quiet band an open file and its parts sit on, one rect per
/// grid row the run spans: `tiles` are the run's tile rects, in grid order.
pub fn paint_parts_band(ui: &mut dyn UiBuilder, tiles: &[[f32; 4]], theme: &UiTheme) {
    let mut row: Option<[f32; 4]> = None;
    let flush = |ui: &mut dyn UiBuilder, rect: [f32; 4]| {
        let pad = 3.0;
        ui.paint_rect_filled(
            [rect[0] - pad, rect[1] - pad],
            [rect[2] + 2.0 * pad, rect[3] + 2.0 * pad],
            with_alpha(theme.surface_elevated, 0.28),
            8.0,
        );
    };
    for &[x, y, w, h] in tiles {
        row = match row {
            Some([rx, ry, _, rh]) if (ry - y).abs() < 0.5 => Some([rx, ry, x + w - rx, rh]),
            Some(done) => {
                flush(ui, done);
                Some([x, y, w, h])
            }
            None => Some([x, y, w, h]),
        };
    }
    if let Some(done) = row {
        flush(ui, done);
    }
}
