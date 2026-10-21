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
use khora_core::ui::theme::{ColorScheme, UiTheme};

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

/// The light counterpart of [`pal`]: the same deep-navy family (hue 245–264)
/// turned into a silver field with navy ink, for the hub — the doorway that
/// stands as the editor's negative.
///
/// Values are `#rrggbb / 255` sRGB (see the module note on the color space).
/// The axes have no light variant: they are editor-only slots and keep the
/// [`pal`] tints.
pub mod pal_light {
    use khora_core::math::LinearRgba;

    // ── Surfaces (silver, hue 245–264) ─────────────────
    /// `#dfe6ee` — the darkest step: wells and the app void.
    pub const BG: LinearRgba = LinearRgba::new(223.0 / 255.0, 230.0 / 255.0, 238.0 / 255.0, 1.0);
    /// `#eef1f5` — default panel surface.
    pub const SURFACE: LinearRgba =
        LinearRgba::new(238.0 / 255.0, 241.0 / 255.0, 245.0 / 255.0, 1.0);
    /// `#f8fafc` — elevated surface, the lightest step.
    pub const SURFACE2: LinearRgba =
        LinearRgba::new(248.0 / 255.0, 250.0 / 255.0, 252.0 / 255.0, 1.0);
    /// `#e3e9f0` — interactive rest (buttons, combos).
    pub const SURFACE3: LinearRgba =
        LinearRgba::new(227.0 / 255.0, 233.0 / 255.0, 240.0 / 255.0, 1.0);
    /// `#d3dce7` — interactive active / selected fill.
    pub const SURFACE_ACTIVE: LinearRgba =
        LinearRgba::new(211.0 / 255.0, 220.0 / 255.0, 231.0 / 255.0, 1.0);

    // ── Lines / borders (navy ink, translucent) ────────
    /// Inner separator — `#141b28` at 8 %.
    pub const SEPARATOR: LinearRgba =
        LinearRgba::new(20.0 / 255.0, 27.0 / 255.0, 40.0 / 255.0, 0.08);
    /// Default panel border — `#141b28` at 14 %.
    pub const BORDER: LinearRgba = LinearRgba::new(20.0 / 255.0, 27.0 / 255.0, 40.0 / 255.0, 0.14);
    /// Strong border for inputs, modals, popups — `#141b28` at 28 %.
    pub const BORDER_STRONG: LinearRgba =
        LinearRgba::new(20.0 / 255.0, 27.0 / 255.0, 40.0 / 255.0, 0.28);

    // ── Text ramp ──────────────────────────────────────
    /// `#141b28` — primary text.
    pub const TEXT: LinearRgba = LinearRgba::new(20.0 / 255.0, 27.0 / 255.0, 40.0 / 255.0, 1.0);
    /// `#3b4759` — secondary text (labels, sub-titles).
    pub const TEXT_DIM: LinearRgba = LinearRgba::new(59.0 / 255.0, 71.0 / 255.0, 89.0 / 255.0, 1.0);
    /// `#4a5668` — muted text (hints, captions). Dark enough to stay above
    /// 4.5:1 on a tinted plate over the background, not only on bare surfaces.
    pub const TEXT_MUTED: LinearRgba =
        LinearRgba::new(74.0 / 255.0, 86.0 / 255.0, 104.0 / 255.0, 1.0);
    /// `#97a3b1` — disabled text.
    pub const TEXT_DISABLED: LinearRgba =
        LinearRgba::new(151.0 / 255.0, 163.0 / 255.0, 177.0 / 255.0, 1.0);
    /// `#eef1f5` — light ink on a filled navy primary surface.
    pub const TEXT_INVERSE: LinearRgba =
        LinearRgba::new(238.0 / 255.0, 241.0 / 255.0, 245.0 / 255.0, 1.0);

    // ── Brand navy ─────────────────────────────────────
    /// `#1d2a40` — primary brand navy (focus, links, primary fills).
    pub const PRIMARY: LinearRgba = LinearRgba::new(29.0 / 255.0, 42.0 / 255.0, 64.0 / 255.0, 1.0);
    /// `#4f5f75` — dimmer navy (resting brand strokes, secondary icons).
    pub const PRIMARY_DIM: LinearRgba =
        LinearRgba::new(79.0 / 255.0, 95.0 / 255.0, 117.0 / 255.0, 1.0);

    // ── Accents — one job each ─────────────────────────
    /// `#5b48c2` — violet: custom / extension agents.
    pub const ACCENT_VIOLET: LinearRgba =
        LinearRgba::new(91.0 / 255.0, 72.0 / 255.0, 194.0 / 255.0, 1.0);
    /// `#065368` — cyan: info / hyperlink.
    pub const ACCENT_CYAN: LinearRgba =
        LinearRgba::new(6.0 / 255.0, 83.0 / 255.0, 104.0 / 255.0, 1.0);
    /// `#8a5e16` — deeper gold: selection / "now" on a light field, still 3:1 on
    /// the active surface a selection bar sits on.
    pub const ACCENT_GOLD: LinearRgba =
        LinearRgba::new(138.0 / 255.0, 94.0 / 255.0, 22.0 / 255.0, 1.0);

    // ── Status ─────────────────────────────────────────
    // A status colour labels a chip painted with a 14% plate of itself, so each
    // keeps 5:1 against that plate over the background.
    /// `#155b35` — success / healthy.
    pub const SUCCESS: LinearRgba = LinearRgba::new(21.0 / 255.0, 91.0 / 255.0, 53.0 / 255.0, 1.0);
    /// `#6b4500` — warning / in-progress.
    pub const WARNING: LinearRgba = LinearRgba::new(107.0 / 255.0, 69.0 / 255.0, 0.0 / 255.0, 1.0);
    /// `#962018` — error / destructive.
    pub const ERROR: LinearRgba = LinearRgba::new(150.0 / 255.0, 32.0 / 255.0, 24.0 / 255.0, 1.0);
}

/// Returns the Khora "Deep Navy / Silver" theme — the editor's. The hub wears
/// its negative, [`khora_light`].
pub fn khora_dark() -> UiTheme {
    UiTheme {
        scheme: ColorScheme::Dark,
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

/// Returns the light Khora theme — silver surfaces, navy ink — built from
/// [`pal_light`], with the same radii, type sizes and spacing as
/// [`khora_dark`].
pub fn khora_light() -> UiTheme {
    let dark = khora_dark();
    UiTheme {
        scheme: ColorScheme::Light,
        // ── Surfaces ──
        background: rgba(pal_light::BG),
        surface: rgba(pal_light::SURFACE),
        surface_elevated: rgba(pal_light::SURFACE2),
        surface_interactive: rgba(pal_light::SURFACE3),
        surface_active: rgba(pal_light::SURFACE_ACTIVE),
        // ── Lines ──
        separator: rgba(pal_light::SEPARATOR),
        border: rgba(pal_light::BORDER),
        border_strong: rgba(pal_light::BORDER_STRONG),
        // ── Text ──
        text: rgba(pal_light::TEXT),
        text_dim: rgba(pal_light::TEXT_DIM),
        text_muted: rgba(pal_light::TEXT_MUTED),
        text_disabled: rgba(pal_light::TEXT_DISABLED),
        text_inverse: rgba(pal_light::TEXT_INVERSE),
        // ── Brand / accents ──
        primary: rgba(pal_light::PRIMARY),
        primary_dim: rgba(pal_light::PRIMARY_DIM),
        accent_a: rgba(pal_light::ACCENT_VIOLET),
        accent_b: rgba(pal_light::ACCENT_CYAN),
        accent_c: rgba(pal_light::ACCENT_GOLD),
        // ── Status ──
        success: rgba(pal_light::SUCCESS),
        warning: rgba(pal_light::WARNING),
        error: rgba(pal_light::ERROR),
        // Axes, radii, type sizes and spacing are the dark theme's.
        ..dark
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

    // ── The light theme ────────────────────────────────

    /// WCAG relative luminance of an sRGB `[r, g, b, _]` slot.
    fn relative_luminance(s: [f32; 4]) -> f32 {
        let lin = |c: f32| {
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(s[0]) + 0.7152 * lin(s[1]) + 0.0722 * lin(s[2])
    }

    /// `fg` composited over the opaque `bg` (sRGB alpha blend).
    fn over(fg: [f32; 4], bg: [f32; 4]) -> [f32; 4] {
        let a = fg[3];
        [
            fg[0] * a + bg[0] * (1.0 - a),
            fg[1] * a + bg[1] * (1.0 - a),
            fg[2] * a + bg[2] * (1.0 - a),
            1.0,
        ]
    }

    /// WCAG contrast ratio of `fg` drawn on the opaque `bg`.
    fn contrast(fg: [f32; 4], bg: [f32; 4]) -> f32 {
        let a = relative_luminance(over(fg, bg));
        let b = relative_luminance(bg);
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    fn assert_contrast(name: &str, fg: [f32; 4], bg: [f32; 4], min: f32) {
        let ratio = contrast(fg, bg);
        assert!(ratio >= min, "{name}: contrast {ratio:.2} < {min}");
    }

    #[test]
    fn contrast_helper_matches_wcag_extremes() {
        let black = [0.0, 0.0, 0.0, 1.0];
        let white = [1.0, 1.0, 1.0, 1.0];
        assert!((contrast(black, white) - 21.0).abs() < 1e-3);
        assert!((contrast(white, white) - 1.0).abs() < 1e-6);
        // A fully transparent ink is the background itself.
        assert!((contrast([0.0, 0.0, 0.0, 0.0], white) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn khora_dark_is_a_dark_scheme() {
        assert_eq!(khora_dark().scheme, ColorScheme::Dark);
    }

    #[test]
    fn khora_light_is_a_light_scheme() {
        assert_eq!(khora_light().scheme, ColorScheme::Light);
    }

    /// The void sits below the panel surface, and the elevated surface is the
    /// lightest step of the ladder — on a light field, elevation reads as
    /// "closer to white".
    #[test]
    fn khora_light_surface_ladder_rises_from_void_to_elevated() {
        let t = khora_light();
        let void = relative_luminance(t.background);
        let surface = relative_luminance(t.surface);
        let elevated = relative_luminance(t.surface_elevated);
        assert!(void < surface, "void {void} !< surface {surface}");
        assert!(
            surface < elevated,
            "surface {surface} !< elevated {elevated}"
        );
        for (name, slot) in [
            ("background", t.background),
            ("surface", t.surface),
            ("surface_interactive", t.surface_interactive),
            ("surface_active", t.surface_active),
        ] {
            assert!(
                relative_luminance(slot) < elevated,
                "{name} is not darker than surface_elevated"
            );
        }
    }

    /// The text ramp lightens from primary text to disabled — hierarchy on a
    /// light field runs the other way from the dark theme.
    #[test]
    fn khora_light_text_ramp_lightens_from_text_to_disabled() {
        let t = khora_light();
        let ramp = [t.text, t.text_dim, t.text_muted, t.text_disabled];
        for w in ramp.windows(2) {
            assert!(
                relative_luminance(w[0]) < relative_luminance(w[1]),
                "text ramp not lightening: {:?} !< {:?}",
                w[0],
                w[1]
            );
        }
    }

    #[test]
    fn khora_light_text_reaches_aaa_contrast_on_the_surface() {
        let t = khora_light();
        assert_contrast("text / surface", t.text, t.surface, 7.0);
    }

    #[test]
    fn khora_light_dim_text_reaches_aa_contrast_on_the_surface() {
        let t = khora_light();
        assert_contrast("text_dim / surface", t.text_dim, t.surface, 4.5);
    }

    #[test]
    fn khora_light_status_and_gold_read_as_graphics_on_the_surface() {
        let t = khora_light();
        for (name, slot) in [
            ("success", t.success),
            ("warning", t.warning),
            ("error", t.error),
            ("accent_c", t.accent_c),
        ] {
            assert_contrast(name, slot, t.surface, 3.0);
        }
    }

    /// The gold selection bar of a selected row sits on the active surface,
    /// the darkest of the interactive fills.
    #[test]
    fn khora_light_gold_selection_reads_on_the_active_surface() {
        let t = khora_light();
        assert_contrast(
            "accent_c / surface_active",
            t.accent_c,
            t.surface_active,
            3.0,
        );
    }

    #[test]
    fn khora_light_inverse_text_reaches_aa_contrast_on_the_primary_fill() {
        let t = khora_light();
        assert_contrast("text_inverse / primary", t.text_inverse, t.primary, 4.5);
    }

    /// The light theme is a palette swap, not a re-layout: radii, type sizes
    /// and spacing match the dark theme, and the editor-only axis tints stay.
    #[test]
    fn khora_light_keeps_the_dark_sizes_and_axes() {
        let (l, d) = (khora_light(), khora_dark());
        assert_eq!(
            [l.radius_sm, l.radius_md, l.radius_lg, l.radius_xl],
            [d.radius_sm, d.radius_md, d.radius_lg, d.radius_xl]
        );
        assert_eq!(
            [
                l.font_size_caption,
                l.font_size_body,
                l.font_size_title,
                l.font_size_display
            ],
            [
                d.font_size_caption,
                d.font_size_body,
                d.font_size_title,
                d.font_size_display
            ]
        );
        assert_eq!([l.pad_row, l.pad_card], [d.pad_row, d.pad_card]);
        assert_eq!(
            [l.axis_x, l.axis_y, l.axis_z],
            [d.axis_x, d.axis_y, d.axis_z]
        );
    }

    /// The light slots carry the light palette, not the dark one.
    #[test]
    fn khora_light_fills_its_slots_from_the_light_palette() {
        let t = khora_light();
        assert_eq!(t.background, rgba(pal_light::BG));
        assert_eq!(t.surface, rgba(pal_light::SURFACE));
        assert_eq!(t.surface_elevated, rgba(pal_light::SURFACE2));
        assert_eq!(t.surface_interactive, rgba(pal_light::SURFACE3));
        assert_eq!(t.surface_active, rgba(pal_light::SURFACE_ACTIVE));
        assert_eq!(t.separator, rgba(pal_light::SEPARATOR));
        assert_eq!(t.border, rgba(pal_light::BORDER));
        assert_eq!(t.border_strong, rgba(pal_light::BORDER_STRONG));
        assert_eq!(t.text, rgba(pal_light::TEXT));
        assert_eq!(t.text_dim, rgba(pal_light::TEXT_DIM));
        assert_eq!(t.text_muted, rgba(pal_light::TEXT_MUTED));
        assert_eq!(t.text_disabled, rgba(pal_light::TEXT_DISABLED));
        assert_eq!(t.text_inverse, rgba(pal_light::TEXT_INVERSE));
        assert_eq!(t.primary, rgba(pal_light::PRIMARY));
        assert_eq!(t.primary_dim, rgba(pal_light::PRIMARY_DIM));
        assert_eq!(t.accent_a, rgba(pal_light::ACCENT_VIOLET));
        assert_eq!(t.accent_b, rgba(pal_light::ACCENT_CYAN));
        assert_eq!(t.accent_c, rgba(pal_light::ACCENT_GOLD));
        assert_eq!(t.success, rgba(pal_light::SUCCESS));
        assert_eq!(t.warning, rgba(pal_light::WARNING));
        assert_eq!(t.error, rgba(pal_light::ERROR));
    }

    /// Every light slot is well-formed (finite, in `[0, 1]`).
    #[test]
    fn khora_light_components_in_range() {
        let t = khora_light();
        for s in [
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
        ] {
            for ch in s {
                assert!(ch.is_finite() && (0.0..=1.0).contains(&ch), "{ch}");
            }
        }
    }
}
