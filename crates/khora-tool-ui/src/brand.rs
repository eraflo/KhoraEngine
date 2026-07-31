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

//! Khora brand tokens — the single source of truth for the "Deep Navy /
//! Silver" palette shared by **every** Khora *tool* surface (editor, hub, and
//! future tools), so they cannot drift.
//!
//! This is cosmetics, not engine contract: it lives here rather than in
//! `khora-core` so that a game built on Khora never inherits the engine
//! vendor's brand. See the crate docs for the split.
//!
//! ## Color space — the stored floats are sRGB, not linear
//!
//! The only place a [`UiTheme`] color reaches egui is
//! `khora-infra/src/ui/egui/theme.rs::c()`, which does `(x * 255.0) as u8`
//! straight into `Color32::from_rgba_unmultiplied` — i.e. it treats the four
//! floats as **sRGB** components in `0..1`. So every value below is
//! `sRGB_byte / 255`, NOT linear-light. (The [`LinearRgba`] type name is a
//! historical misnomer in this context; here it is a plain `[r,g,b,a]` sRGB
//! carrier.)
//!
//! ## Provenance
//!
//! Each constant is the offline `oklch(L C H)` → sRGB conversion of the token
//! of the same name in the design mockups (`design-reference/*.html`). The
//! full table and the frozen conversion procedure live in
//! `design-reference/TOKENS.md`; the values there are verified byte-for-byte
//! against Chrome's `oklch()` rasterization.

use khora_core::math::LinearRgba;
use khora_core::ui::theme::UiTheme;

/// Named palette constants. Widget code should take a [`UiTheme`] and read its
/// semantic slots; reach into `pal` directly only inside the theme producer
/// or where a raw brand color is genuinely needed (e.g. axis tints).
pub mod pal {
    use khora_core::math::LinearRgba;

    // ── Surfaces (deep navy, hue 264) ──────────────────
    /// `bg-0` — app void, behind everything.
    pub const BG: LinearRgba = LinearRgba::new(0.0314, 0.0471, 0.0824, 1.0);
    /// `bg-1` — default panel surface.
    pub const SURFACE: LinearRgba = LinearRgba::new(0.0588, 0.0784, 0.1176, 1.0);
    /// `bg-2` — elevated / rows / tooltips.
    pub const SURFACE2: LinearRgba = LinearRgba::new(0.0824, 0.1059, 0.1569, 1.0);
    /// `bg-3` — interactive rest (buttons, combos).
    pub const SURFACE3: LinearRgba = LinearRgba::new(0.1176, 0.1451, 0.2039, 1.0);
    /// `bg-4` — interactive active / selected fill.
    pub const SURFACE_ACTIVE: LinearRgba = LinearRgba::new(0.1608, 0.1961, 0.2627, 1.0);

    // ── Lines / borders ────────────────────────────────
    /// Inner separator (`--line` at reduced alpha).
    pub const SEPARATOR: LinearRgba = LinearRgba::new(0.1961, 0.2196, 0.2667, 0.5);
    /// Default panel border (`--line`).
    pub const BORDER: LinearRgba = LinearRgba::new(0.1961, 0.2196, 0.2667, 0.65);
    /// Strong border for inputs, modals, popups (`--line-strong`).
    pub const BORDER_LIGHT: LinearRgba = LinearRgba::new(0.3137, 0.3451, 0.4078, 0.85);
    /// Alias of [`BORDER`] using the mockup's `--line` name.
    pub const LINE: LinearRgba = BORDER;
    /// Alias of [`BORDER_LIGHT`] using the mockup's `--line-strong` name.
    pub const LINE_STRONG: LinearRgba = BORDER_LIGHT;

    // ── Brand silver (hue 245) ─────────────────────────
    /// Primary brand silver (focus, links, active highlights).
    pub const PRIMARY: LinearRgba = LinearRgba::new(0.7490, 0.8314, 0.9059, 1.0);
    /// Dimmer silver (resting brand strokes, secondary icons).
    pub const PRIMARY_DIM: LinearRgba = LinearRgba::new(0.5412, 0.6353, 0.7137, 1.0);
    /// Semantic alias of [`PRIMARY`].
    pub const SILVER: LinearRgba = PRIMARY;
    /// Semantic alias of [`PRIMARY_DIM`].
    pub const SILVER_DIM: LinearRgba = PRIMARY_DIM;

    // ── Accents — one job each ─────────────────────────
    /// Violet — custom / extension agents.
    pub const ACCENT_VIOLET: LinearRgba = LinearRgba::new(0.6784, 0.6078, 0.9647, 1.0);
    /// Cyan — info / hyperlink.
    pub const ACCENT_CYAN: LinearRgba = LinearRgba::new(0.4510, 0.8000, 0.9176, 1.0);
    /// Gold — **selection / active only**.
    pub const ACCENT_GOLD: LinearRgba = LinearRgba::new(0.9059, 0.7451, 0.4078, 1.0);
    /// Legacy alias for the violet accent.
    pub const ACCENT: LinearRgba = ACCENT_VIOLET;
    /// Semantic alias of [`ACCENT_VIOLET`].
    pub const VIOLET: LinearRgba = ACCENT_VIOLET;
    /// Semantic alias of [`ACCENT_CYAN`].
    pub const CYAN: LinearRgba = ACCENT_CYAN;
    /// Semantic alias of [`ACCENT_GOLD`].
    pub const GOLD: LinearRgba = ACCENT_GOLD;

    // ── Status ─────────────────────────────────────────
    /// Green — success / healthy.
    pub const SUCCESS: LinearRgba = LinearRgba::new(0.4471, 0.8118, 0.5569, 1.0);
    /// Amber — warning / in-progress.
    pub const WARNING: LinearRgba = LinearRgba::new(0.9529, 0.6824, 0.3176, 1.0);
    /// Red — error / destructive.
    pub const ERROR: LinearRgba = LinearRgba::new(0.9647, 0.4275, 0.4039, 1.0);
    /// Semantic alias of [`SUCCESS`].
    pub const GREEN: LinearRgba = SUCCESS;
    /// Semantic alias of [`WARNING`].
    pub const AMBER: LinearRgba = WARNING;
    /// Semantic alias of [`ERROR`].
    pub const RED: LinearRgba = ERROR;

    // ── Text ramp ──────────────────────────────────────
    /// `ink-0` — primary text.
    pub const TEXT: LinearRgba = LinearRgba::new(0.9451, 0.9569, 0.9686, 1.0);
    /// `ink-1` — secondary text (labels, sub-titles).
    pub const TEXT_DIM: LinearRgba = LinearRgba::new(0.7490, 0.7725, 0.7922, 1.0);
    /// `ink-2` — muted text (hints, captions).
    pub const TEXT_MUTED: LinearRgba = LinearRgba::new(0.5490, 0.5765, 0.6039, 1.0);
    /// `ink-3` — disabled text.
    pub const TEXT_DISABLED: LinearRgba = LinearRgba::new(0.3647, 0.3922, 0.4235, 1.0);
    /// Dark ink used on top of a filled silver surface (primary buttons).
    pub const TEXT_INVERSE: LinearRgba = LinearRgba::new(0.0510, 0.0706, 0.1059, 1.0);

    // ── 3D axes (X = red, Y = green, Z = cyan) ─────────
    /// X-axis tint (shares [`RED`]).
    pub const AXIS_X: LinearRgba = RED;
    /// Y-axis tint (shares [`GREEN`]).
    pub const AXIS_Y: LinearRgba = GREEN;
    /// Z-axis tint (shares [`CYAN`]).
    pub const AXIS_Z: LinearRgba = CYAN;
}

/// Returns the canonical Khora "Deep Navy / Silver" theme. Both the editor and
/// the hub install this exact [`UiTheme`]; there is no per-app variant.
pub fn khora_dark() -> UiTheme {
    UiTheme {
        // ── Surfaces ──
        background: rgba(pal::BG),
        surface: rgba(pal::SURFACE),
        surface_elevated: rgba(pal::SURFACE2),
        surface_interactive: rgba(pal::SURFACE3),
        surface_active: rgba(pal::SURFACE_ACTIVE),
        // ── Lines ──
        separator: rgba(pal::SEPARATOR),
        border: rgba(pal::BORDER),
        border_strong: rgba(pal::BORDER_LIGHT),
        // ── Text ──
        text: rgba(pal::TEXT),
        text_dim: rgba(pal::TEXT_DIM),
        text_muted: rgba(pal::TEXT_MUTED),
        text_disabled: rgba(pal::TEXT_DISABLED),
        text_inverse: rgba(pal::TEXT_INVERSE),
        // ── Brand / accents ──
        primary: rgba(pal::PRIMARY),
        primary_dim: rgba(pal::PRIMARY_DIM),
        accent_a: rgba(pal::ACCENT_VIOLET),
        accent_b: rgba(pal::ACCENT_CYAN),
        accent_c: rgba(pal::ACCENT_GOLD),
        // ── Status ──
        success: rgba(pal::SUCCESS),
        warning: rgba(pal::WARNING),
        error: rgba(pal::ERROR),
        // ── 3D axes ──
        axis_x: rgba(pal::AXIS_X),
        axis_y: rgba(pal::AXIS_Y),
        axis_z: rgba(pal::AXIS_Z),
        // ── Sizing tokens ──
        radius_sm: 3.0,
        radius_md: 5.0,
        radius_lg: 7.0,
        radius_xl: 10.0,
        // ── Type sizes ──
        font_size_caption: 10.5,
        font_size_body: 12.5,
        font_size_title: 14.0,
        font_size_display: 22.0,
        // ── Spacing ──
        pad_row: 8.0,
        pad_card: 16.0,
    }
}

/// Converts a [`LinearRgba`] into the `[f32; 4]` form taken by [`UiTheme`]
/// slots and `UiBuilder` paint methods.
#[inline]
pub fn rgba(c: LinearRgba) -> [f32; 4] {
    [c.r, c.g, c.b, c.a]
}

/// Returns a copy of `c` with its alpha replaced by `a` (clamped to `0..=1`).
#[inline]
pub fn with_alpha(c: LinearRgba, a: f32) -> LinearRgba {
    LinearRgba::new(c.r, c.g, c.b, a.clamp(0.0, 1.0))
}

/// Multiplies the alpha channel of `c` by `factor` (clamped to `0..=1`).
#[inline]
pub fn tint(c: LinearRgba, factor: f32) -> LinearRgba {
    LinearRgba::new(c.r, c.g, c.b, c.a * factor.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Relative luminance of an sRGB-ish `[r,g,b,a]` slot (alpha ignored).
    fn luma(s: [f32; 4]) -> f32 {
        0.2126 * s[0] + 0.7152 * s[1] + 0.0722 * s[2]
    }

    /// The brand theme is visibly distinct from the neutral default (migrated
    /// from the old editor `theme.rs`).
    #[test]
    fn khora_dark_overrides_default_palette() {
        let dflt = UiTheme::default();
        let khora = khora_dark();
        assert!(
            khora.background[2] < dflt.background[2] || khora.background[0] < dflt.background[0]
        );
        assert!(khora.primary[0] > 0.50);
        assert!(khora.primary[1] > 0.50);
        assert!(khora.primary[2] > 0.50);
    }

    /// All color slots are well-formed (finite, in `[0, 1]`).
    #[test]
    fn all_components_in_range() {
        let t = khora_dark();
        let slots: [[f32; 4]; 24] = [
            t.background,
            t.surface,
            t.surface_elevated,
            t.surface_interactive,
            t.surface_active,
            t.separator,
            t.border,
            t.border_strong,
            t.text,
            t.text_dim,
            t.text_muted,
            t.text_disabled,
            t.text_inverse,
            t.primary,
            t.primary_dim,
            t.accent_a,
            t.accent_b,
            t.accent_c,
            t.success,
            t.warning,
            t.error,
            t.axis_x,
            t.axis_y,
            t.axis_z,
        ];
        for s in slots {
            for ch in s {
                assert!(ch.is_finite(), "non-finite component");
                assert!((0.0..=1.0).contains(&ch), "out-of-range component: {ch}");
            }
        }
    }

    /// The surface ladder gets monotonically lighter from void to active, and
    /// the text ramp gets monotonically darker from primary to disabled — the
    /// two invariants every panel relies on for depth and hierarchy.
    #[test]
    fn ladders_are_monotonic() {
        let t = khora_dark();
        let surfaces = [
            t.background,
            t.surface,
            t.surface_elevated,
            t.surface_interactive,
            t.surface_active,
        ];
        for w in surfaces.windows(2) {
            assert!(
                luma(w[0]) < luma(w[1]),
                "surface ladder not increasing: {} !< {}",
                luma(w[0]),
                luma(w[1])
            );
        }
        let text = [t.text, t.text_dim, t.text_muted, t.text_disabled];
        for w in text.windows(2) {
            assert!(
                luma(w[0]) > luma(w[1]),
                "text ramp not decreasing: {} !> {}",
                luma(w[0]),
                luma(w[1])
            );
        }
    }

    /// Corner radii are strictly increasing sm → md → lg → xl.
    #[test]
    fn radii_increasing() {
        let t = khora_dark();
        assert!(t.radius_sm < t.radius_md);
        assert!(t.radius_md < t.radius_lg);
        assert!(t.radius_lg < t.radius_xl);
    }

    /// `text_inverse` is dark (for use on the light silver primary fill) while
    /// `text` is light — they must sit on opposite ends of the ramp.
    #[test]
    fn text_inverse_contrasts_primary() {
        let t = khora_dark();
        assert!(luma(t.text_inverse) < 0.2, "inverse ink should be dark");
        assert!(luma(t.text) > 0.8, "primary text should be light");
    }
}
