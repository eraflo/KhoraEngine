// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Khora Hub palette — produces a [`UiTheme`] from OKLCH values that
//! match the editor and the mdBook documentation.
//!
//! Mirrors the editor's "Deep Navy / Silver" brand language but stays
//! a pure value producer: the engine's `AppContext::set_theme` does
//! the work of lifting it onto the egui backend.

use khora_sdk::tool_ui::{LinearRgba, UiTheme};

/// Linear-RGB dark palette, matching Khora brand. Each constant is
/// computed from the same OKLCH coordinates that drive the editor's
/// `khora_dark()` and the mdBook CSS — single source of truth.
pub mod pal {
    use khora_sdk::tool_ui::LinearRgba;

    // ── Surfaces (deep navy, oklch 0.16–0.32 hue 265) ──
    /// Outermost background — what panels sit on.
    pub const BG: LinearRgba = LinearRgba::new(0.0179, 0.0193, 0.0273, 1.0);
    /// Default panel surface.
    pub const SURFACE: LinearRgba = LinearRgba::new(0.0290, 0.0312, 0.0426, 1.0);
    /// Slightly elevated surface (cards, tooltips, table headers).
    pub const SURFACE2: LinearRgba = LinearRgba::new(0.0419, 0.0451, 0.0608, 1.0);
    /// Hover / interactive surface.
    pub const SURFACE3: LinearRgba = LinearRgba::new(0.0584, 0.0625, 0.0826, 1.0);
    /// Selected / pressed surface.
    pub const SURFACE_ACTIVE: LinearRgba = LinearRgba::new(0.0876, 0.0928, 0.1207, 1.0);

    // ── Borders / separators ──
    /// Default panel border (semi-transparent).
    pub const BORDER: LinearRgba = LinearRgba::new(0.1071, 0.1149, 0.1496, 0.7);
    /// Stronger / hover border.
    pub const BORDER_LIGHT: LinearRgba = LinearRgba::new(0.1786, 0.1907, 0.2455, 1.0);
    /// Inline separator inside a panel (more transparent).
    pub const SEPARATOR: LinearRgba = LinearRgba::new(0.0701, 0.0760, 0.1010, 0.55);

    // ── Brand silver — the slightly bluish hue 240 from mdBook ──
    /// Primary brand silver (focus, links, active highlights).
    pub const PRIMARY: LinearRgba = LinearRgba::new(0.6494, 0.6776, 0.7263, 1.0);
    /// Dimmer silver (resting state of brand-coloured strokes).
    pub const PRIMARY_DIM: LinearRgba = LinearRgba::new(0.3815, 0.4070, 0.4533, 1.0);

    // ── Accents ──
    /// Violet — extension / custom-mode signal.
    pub const ACCENT_VIOLET: LinearRgba = LinearRgba::new(0.4795, 0.3370, 0.7029, 1.0);
    /// Cyan — informational / hyperlink.
    pub const ACCENT_CYAN: LinearRgba = LinearRgba::new(0.3796, 0.6242, 0.7619, 1.0);
    /// Gold — selection, attention.
    pub const ACCENT_GOLD: LinearRgba = LinearRgba::new(0.7351, 0.5410, 0.0945, 1.0);
    /// Legacy alias for the violet accent.
    pub const ACCENT: LinearRgba = ACCENT_VIOLET;

    // ── Status ──
    /// Green — success, healthy.
    pub const SUCCESS: LinearRgba = LinearRgba::new(0.2912, 0.6726, 0.4233, 1.0);
    /// Amber — warning, in-progress.
    pub const WARNING: LinearRgba = LinearRgba::new(0.7351, 0.5197, 0.1156, 1.0);
    /// Red — error.
    pub const ERROR: LinearRgba = LinearRgba::new(0.5755, 0.0822, 0.0451, 1.0);

    // ── Text ──
    /// Primary text colour.
    pub const TEXT: LinearRgba = LinearRgba::new(0.9091, 0.9123, 0.9162, 1.0);
    /// Secondary text colour (labels, sub-titles).
    pub const TEXT_DIM: LinearRgba = LinearRgba::new(0.6418, 0.6471, 0.6535, 1.0);
    /// Tertiary text colour (hints, captions).
    pub const TEXT_MUTED: LinearRgba = LinearRgba::new(0.3744, 0.3815, 0.3902, 1.0);
    /// Disabled text colour.
    pub const TEXT_DISABLED: LinearRgba = LinearRgba::new(0.2009, 0.2074, 0.2156, 1.0);
}

/// Returns the Khora Hub theme as a backend-neutral [`UiTheme`]. Hand
/// it to `AppContext::set_theme` once at startup.
pub fn khora_hub_dark() -> UiTheme {
    UiTheme {
        // ── Surfaces ──
        background: as_array(pal::BG),
        surface: as_array(pal::SURFACE),
        surface_elevated: as_array(pal::SURFACE2),
        surface_interactive: as_array(pal::SURFACE3),
        surface_active: as_array(pal::SURFACE_ACTIVE),
        // ── Lines ──
        separator: as_array(pal::SEPARATOR),
        border: as_array(pal::BORDER),
        border_strong: as_array(pal::BORDER_LIGHT),
        // ── Text ──
        text: as_array(pal::TEXT),
        text_dim: as_array(pal::TEXT_DIM),
        text_muted: as_array(pal::TEXT_MUTED),
        text_disabled: as_array(pal::TEXT_DISABLED),
        // ── Brand ──
        primary: as_array(pal::PRIMARY),
        primary_dim: as_array(pal::PRIMARY_DIM),
        accent_a: as_array(pal::ACCENT_VIOLET),
        accent_b: as_array(pal::ACCENT_CYAN),
        accent_c: as_array(pal::ACCENT_GOLD),
        // ── Status ──
        success: as_array(pal::SUCCESS),
        warning: as_array(pal::WARNING),
        error: as_array(pal::ERROR),
        // ── 3D axes (unused by hub, but theme requires them) ──
        axis_x: as_array(LinearRgba::new(0.6342, 0.0951, 0.0533, 1.0)),
        axis_y: as_array(LinearRgba::new(0.4324, 0.6810, 0.2391, 1.0)),
        axis_z: as_array(LinearRgba::new(0.2509, 0.4317, 0.7619, 1.0)),
        // ── Sizing tokens ──
        radius_sm: 4.0,
        radius_md: 5.0,
        radius_lg: 8.0,
        radius_xl: 12.0,
        // ── Type sizes ──
        font_size_caption: 11.0,
        font_size_body: 12.0,
        font_size_title: 14.0,
        font_size_display: 18.0,
        // ── Spacing ──
        pad_row: 6.0,
        pad_card: 14.0,
    }
}

#[inline]
fn as_array(c: LinearRgba) -> [f32; 4] {
    [c.r, c.g, c.b, c.a]
}

/// Converts a [`LinearRgba`] into the `[f32; 4]` form taken by
/// [`UiBuilder`](khora_sdk::tool_ui::UiBuilder) paint methods.
pub fn rgba(c: LinearRgba) -> [f32; 4] {
    as_array(c)
}

/// Multiplies the alpha channel of `c` by `factor`. Equivalent to the
/// old `tint()` helper but operates on linear-space colours.
pub fn tint(c: LinearRgba, factor: f32) -> LinearRgba {
    LinearRgba::new(c.r, c.g, c.b, c.a * factor.clamp(0.0, 1.0))
}
