// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Per-corner radius of a rounded rectangle.

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
