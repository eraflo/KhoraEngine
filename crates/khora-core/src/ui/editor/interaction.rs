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

//! What the pointer and the keyboard did to a hot region this frame.

/// Result of an [`UiBuilder::interact_rect`](super::UiBuilder::interact_rect) call.
#[derive(Debug, Clone, Copy, Default)]
pub struct Interaction {
    /// The pointer is on this rect, and on nothing drawn above it: what a
    /// widget tints itself by. A button drawn over the rect takes it.
    pub hovered: bool,
    /// The pointer is inside the rect this frame, on whatever is drawn above
    /// it — what a container shows its own controls by. A row's actions,
    /// revealed while it is pointed at, would otherwise take the row's hover
    /// as the pointer reached them and vanish.
    pub contains_pointer: bool,
    /// Primary button was pressed inside the rect this frame.
    pub clicked: bool,
    /// Primary button is currently held inside the rect.
    pub pressed: bool,
    /// Pointer double-clicked inside the rect this frame.
    pub double_clicked: bool,
    /// This widget holds keyboard focus.
    ///
    /// Without it no widget can draw a focus ring, so a keyboard user has no
    /// way to see where they are — which is why the editor's keyboard story
    /// could not be built before this existed.
    pub focused: bool,
}
