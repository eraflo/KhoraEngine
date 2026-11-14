// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Two-axis alignment, and the one-axis alignment it combines.

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
