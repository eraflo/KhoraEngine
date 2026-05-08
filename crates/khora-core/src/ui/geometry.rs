// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Backend-agnostic UI geometry types — `Margin`, `Stroke`,
//! `CornerRadius`, `Align`, `Align2`.
//!
//! These mirror the same concepts every immediate-mode UI library
//! exposes (egui, iced, druid, …). Apps and widget code reference
//! only these neutral types; the concrete backend in `khora-infra`
//! converts them to its native equivalents at the trait-impl boundary.

use crate::math::LinearRgba;

/// Padding (or outer spacing) on the four sides of a region.
///
/// Values are in logical pixels. Negative values are allowed (useful
/// for overlap effects); backends that don't support them clamp.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Margin {
    /// Top edge.
    pub top: f32,
    /// Bottom edge.
    pub bottom: f32,
    /// Left edge.
    pub left: f32,
    /// Right edge.
    pub right: f32,
}

impl Margin {
    /// Zero margin on every side.
    pub const ZERO: Self = Self {
        top: 0.0,
        bottom: 0.0,
        left: 0.0,
        right: 0.0,
    };

    /// Same value on every side.
    #[inline]
    pub const fn same(value: f32) -> Self {
        Self {
            top: value,
            bottom: value,
            left: value,
            right: value,
        }
    }

    /// Symmetric padding (different `vertical` and `horizontal` values).
    #[inline]
    pub const fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self {
            top: vertical,
            bottom: vertical,
            left: horizontal,
            right: horizontal,
        }
    }
}

/// A border or line stroke — colour + thickness.
///
/// Thickness is in logical pixels. A thickness of `0.0` paints
/// nothing; backends should fast-path that case.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stroke {
    /// Stroke colour.
    pub color: LinearRgba,
    /// Stroke thickness in logical pixels.
    pub width: f32,
}

impl Stroke {
    /// A `None`-equivalent — zero-width transparent stroke. Backends
    /// recognise it and skip painting.
    pub const NONE: Self = Self {
        color: LinearRgba::TRANSPARENT,
        width: 0.0,
    };

    /// Convenience constructor.
    #[inline]
    pub const fn new(color: LinearRgba, width: f32) -> Self {
        Self { color, width }
    }
}

impl Default for Stroke {
    fn default() -> Self {
        Self::NONE
    }
}

/// Per-corner radius of a rounded rectangle, in logical pixels.
///
/// Backends that only support a single radius use the maximum of the
/// four corners (or `nw` if they're all expected equal).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CornerRadius {
    /// North-west (top-left).
    pub nw: f32,
    /// North-east (top-right).
    pub ne: f32,
    /// South-west (bottom-left).
    pub sw: f32,
    /// South-east (bottom-right).
    pub se: f32,
}

impl CornerRadius {
    /// All corners share `value`.
    #[inline]
    pub const fn same(value: f32) -> Self {
        Self {
            nw: value,
            ne: value,
            sw: value,
            se: value,
        }
    }

    /// Square (no rounding).
    pub const ZERO: Self = Self::same(0.0);

    /// Returns the largest of the four corner radii. Useful when a
    /// backend can only express a single uniform corner radius.
    #[inline]
    pub fn max(&self) -> f32 {
        self.nw.max(self.ne).max(self.sw).max(self.se)
    }
}

/// One-axis alignment — `Min` (top/left), `Center`, or `Max` (bottom/right).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    /// Anchor at the start of the axis (top for vertical, left for
    /// horizontal).
    #[default]
    Min,
    /// Anchor in the middle of the axis.
    Center,
    /// Anchor at the end (bottom for vertical, right for horizontal).
    Max,
}

/// Two-axis alignment — combination of horizontal + vertical [`Align`].
///
/// Common variants are exposed as constants
/// (`LEFT_TOP`, `CENTER_CENTER`, `RIGHT_BOTTOM`, …) so call sites read
/// like CSS `text-align` / `vertical-align`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Align2 {
    /// Horizontal alignment.
    pub x: Align,
    /// Vertical alignment.
    pub y: Align,
}

impl Align2 {
    /// Top-left.
    pub const LEFT_TOP: Self = Self {
        x: Align::Min,
        y: Align::Min,
    };
    /// Top, horizontally centered.
    pub const CENTER_TOP: Self = Self {
        x: Align::Center,
        y: Align::Min,
    };
    /// Top-right.
    pub const RIGHT_TOP: Self = Self {
        x: Align::Max,
        y: Align::Min,
    };
    /// Vertically centered, left-aligned.
    pub const LEFT_CENTER: Self = Self {
        x: Align::Min,
        y: Align::Center,
    };
    /// Centered on both axes.
    pub const CENTER_CENTER: Self = Self {
        x: Align::Center,
        y: Align::Center,
    };
    /// Vertically centered, right-aligned.
    pub const RIGHT_CENTER: Self = Self {
        x: Align::Max,
        y: Align::Center,
    };
    /// Bottom-left.
    pub const LEFT_BOTTOM: Self = Self {
        x: Align::Min,
        y: Align::Max,
    };
    /// Bottom, horizontally centered.
    pub const CENTER_BOTTOM: Self = Self {
        x: Align::Center,
        y: Align::Max,
    };
    /// Bottom-right.
    pub const RIGHT_BOTTOM: Self = Self {
        x: Align::Max,
        y: Align::Max,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn margin_helpers() {
        let m = Margin::same(4.0);
        assert_eq!(m.top, 4.0);
        assert_eq!(m.left, 4.0);

        let s = Margin::symmetric(8.0, 6.0);
        assert_eq!(s.top, 6.0);
        assert_eq!(s.bottom, 6.0);
        assert_eq!(s.left, 8.0);
        assert_eq!(s.right, 8.0);
    }

    #[test]
    fn stroke_none_is_invisible() {
        assert_eq!(Stroke::NONE.width, 0.0);
        assert_eq!(Stroke::NONE.color.a, 0.0);
    }

    #[test]
    fn corner_radius_max() {
        let cr = CornerRadius {
            nw: 4.0,
            ne: 8.0,
            sw: 2.0,
            se: 6.0,
        };
        assert_eq!(cr.max(), 8.0);
    }

    #[test]
    fn align2_constants() {
        assert_eq!(Align2::LEFT_TOP.x, Align::Min);
        assert_eq!(Align2::LEFT_TOP.y, Align::Min);
        assert_eq!(Align2::RIGHT_BOTTOM.x, Align::Max);
        assert_eq!(Align2::CENTER_CENTER.y, Align::Center);
    }
}
