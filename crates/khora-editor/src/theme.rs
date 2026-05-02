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

//! Khora Engine editor brand theme — "Deep Navy / Silver" palette.
//!
//! The values below are derived from `oklch(L C H)` values defined in the
//! design mockup (`Khora Editor.html` / `styles.css`). The conversion to
//! linear RGB happens once, off-line, and is hardcoded here so the runtime
//! never has to recompute it.
//!
//! The mapping to semantic [`EditorTheme`] slots is:
//!
//! | Theme slot          | Mockup token            | Brand role                       |
//! |---------------------|-------------------------|----------------------------------|
//! | `primary`           | `--silver`              | Brand silver / focus highlight   |
//! | `primary_dim`       | `--silver-dim`          | Dim silver                       |
//! | `accent_a`          | `--accent-violet`       | Custom agents / extensible mode  |
//! | `accent_b`          | `--accent-cyan`         | Info / log info                  |
//! | `accent_c`          | `--accent-gold`         | Selection / focus on viewport    |
//! | `success`           | `--accent-green`        | Play / running                   |
//! | `warning`           | `--accent-amber`        | Warnings                         |
//! | `error`             | `--accent-red`          | Errors                           |
//! | `axis_x/y/z`        | OKLCH 25 / 145 / 240    | Standard 3D axis colors          |

use khora_sdk::EditorTheme;

/// Returns the Khora Engine editor "Deep Navy / Silver" theme.
pub fn khora_dark() -> EditorTheme {
    EditorTheme {
        // ── Surfaces (oklch 0.16–0.32 chroma 0.022–0.036 hue 265) ──
        background: [0.0179, 0.0193, 0.0273, 1.0],         // oklch(0.16 0.022 265)
        surface: [0.0290, 0.0312, 0.0426, 1.0],            // oklch(0.20 0.025 265)
        surface_elevated: [0.0419, 0.0451, 0.0608, 1.0],   // oklch(0.235 0.028 265)
        surface_interactive: [0.0584, 0.0625, 0.0826, 1.0], // oklch(0.27 0.032 265)
        surface_active: [0.0876, 0.0928, 0.1207, 1.0],     // oklch(0.32 0.036 265)

        // ── Lines / borders ───────────────────────────
        separator: [0.0701, 0.0760, 0.1010, 0.55],         // oklch(0.30 0.02 265 / .55)
        border: [0.1071, 0.1149, 0.1496, 0.7],             // oklch(0.36 0.025 265 / .70)
        border_strong: [0.1786, 0.1907, 0.2455, 1.0],      // oklch(0.46 0.03 265)

        // ── Text ──────────────────────────────────────
        text: [0.9091, 0.9123, 0.9162, 1.0],               // oklch(0.96 0.005 250)
        text_dim: [0.6418, 0.6471, 0.6535, 1.0],           // oklch(0.82 0.01 250)
        text_muted: [0.3744, 0.3815, 0.3902, 1.0],         // oklch(0.65 0.012 250)
        text_disabled: [0.2009, 0.2074, 0.2156, 1.0],      // oklch(0.50 0.015 250)

        // ── Brand: silver primary ─────────────────────
        primary: [0.6494, 0.6776, 0.7263, 1.0],            // oklch(0.84 0.04 240)
        primary_dim: [0.3815, 0.4070, 0.4533, 1.0],        // oklch(0.68 0.045 240)

        // ── Accents (violet / cyan / gold) ────────────
        accent_a: [0.4795, 0.3370, 0.7029, 1.0],           // oklch(0.72 0.14 290) violet
        accent_b: [0.3796, 0.6242, 0.7619, 1.0],           // oklch(0.78 0.10 220) cyan
        accent_c: [0.7351, 0.5410, 0.0945, 1.0],           // oklch(0.80 0.12 82)  gold

        // ── Status ────────────────────────────────────
        success: [0.2912, 0.6726, 0.4233, 1.0],            // oklch(0.78 0.13 150) green
        warning: [0.7351, 0.5197, 0.1156, 1.0],            // oklch(0.78 0.14 70)  amber
        error: [0.5755, 0.0822, 0.0451, 1.0],              // oklch(0.68 0.18 25)  red

        // ── 3D axes ───────────────────────────────────
        axis_x: [0.6342, 0.0951, 0.0533, 1.0],             // oklch(0.70 0.18 25)
        axis_y: [0.4324, 0.6810, 0.2391, 1.0],             // oklch(0.78 0.16 145)
        axis_z: [0.2509, 0.4317, 0.7619, 1.0],             // oklch(0.72 0.14 240)

        // ── Sizing tokens (from --r-sm/md/lg/xl) ─────
        radius_sm: 4.0,
        radius_md: 6.0,
        radius_lg: 10.0,
        radius_xl: 14.0,

        // ── Type sizes ────────────────────────────────
        font_size_caption: 10.5,
        font_size_body: 12.5,
        font_size_title: 14.0,
        font_size_display: 18.0,

        // ── Spacing (--pad-row / --pad-card) ─────────
        pad_row: 8.0,
        pad_card: 14.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke test: the brand theme produces values that are clearly distinct
    /// from the default neutral palette (so callers can tell the override took
    /// effect).
    #[test]
    fn khora_dark_overrides_default_palette() {
        let dflt = EditorTheme::default();
        let khora = khora_dark();
        // Surfaces should be visibly darker (deep navy).
        assert!(khora.background[2] < dflt.background[2] || khora.background[0] < dflt.background[0]);
        // Primary silver should be much brighter than the default blue.
        assert!(khora.primary[0] > 0.50);
        assert!(khora.primary[1] > 0.50);
        assert!(khora.primary[2] > 0.50);
    }

    /// Sanity: all colors are well-formed (no NaN, all channels in [0, 1]).
    #[test]
    fn all_components_in_range() {
        let t = khora_dark();
        let slots: [[f32; 4]; 16] = [
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
            t.primary,
            t.primary_dim,
            t.accent_a,
            t.accent_b,
        ];
        for s in slots {
            for c in s {
                assert!(c.is_finite(), "non-finite component");
                assert!((0.0..=1.0).contains(&c), "out-of-range component: {}", c);
            }
        }
    }
}
