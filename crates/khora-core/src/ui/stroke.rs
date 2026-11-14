// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! A border or line stroke: colour and thickness.

use crate::math::LinearRgba;

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
