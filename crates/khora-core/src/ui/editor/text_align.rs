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

//! Where a styled text is anchored horizontally.

/// Horizontal alignment for [`UiBuilder::paint_text_styled`](super::UiBuilder::paint_text_styled).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    /// Anchor at the left edge.
    Left,
    /// Anchor at the centre.
    Center,
    /// Anchor at the right edge.
    Right,
}
