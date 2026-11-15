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
    /// The pointer is on this rect and on nothing drawn above it.
    ///
    /// What a widget tints itself by; a button drawn over the rect takes it.
    pub hovered: bool,
    /// The pointer is inside the rect this frame, whatever is drawn above it.
    ///
    /// What a container shows its own controls by: a row's actions, revealed
    /// on hover, would otherwise take the row's hover and vanish.
    pub contains_pointer: bool,
    /// Primary button was pressed inside the rect this frame.
    pub clicked: bool,
    /// Primary button is currently held inside the rect.
    pub pressed: bool,
    /// Pointer double-clicked inside the rect this frame.
    pub double_clicked: bool,
    /// This widget holds keyboard focus.
    ///
    /// What a widget draws its focus ring by, so a keyboard user sees where they are.
    pub focused: bool,
}
