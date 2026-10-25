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

//! What an inline text field did this frame.

/// Outcome of an [`UiBuilder::inline_text_field`](super::UiBuilder::inline_text_field) this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InlineEditEvent {
    /// Still editing (or backend has no real text field).
    #[default]
    Idle,
    /// The text changed this frame (not yet committed).
    Changed,
    /// The user confirmed the edit (Enter, or focus lost without Escape).
    Committed,
    /// The user cancelled the edit (Escape).
    Cancelled,
}
