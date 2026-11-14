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

pub use super::align2::{Align, Align2};
pub use super::corner_radius::CornerRadius;
pub use super::stroke::Stroke;

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
