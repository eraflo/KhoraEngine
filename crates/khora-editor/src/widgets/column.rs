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

//! The horizontal span a painter fills.

/// A horizontal span of the screen a painter fills: its left edge and its width.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Column {
    /// The left edge, in screen space.
    pub x: f32,
    /// The width.
    pub w: f32,
}

impl Column {
    /// Returns the span from `x`, `w` wide.
    pub const fn new(x: f32, w: f32) -> Self {
        Self { x, w }
    }
}

#[cfg(test)]
mod tests {
    use super::Column;

    #[test]
    fn a_column_starts_at_its_left_edge_and_spans_its_width() {
        let column = Column::new(10.0, 32.5);
        assert_eq!((column.x, column.w), (10.0, 32.5));
    }
}
