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

//! Which registered face a styled text is painted with.

/// Font family hint passed to
/// [`UiBuilder::paint_text_styled`](super::UiBuilder::paint_text_styled).
/// Backends map each variant to whichever face was registered for that family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontFamilyHint {
    /// The default proportional face: text, labels, titles.
    Proportional,
    /// The monospaced face: code, paths, numbers.
    Monospace,
    /// The display face for screen headings and hero numerals. Falls back to
    /// the proportional family when the installed pack has none.
    Display,
    /// The icon font. Pass single-char codepoints.
    Icons,
    /// The widest cut of the text face, for the product name itself. Falls
    /// back to the display family, then to the proportional one, when the
    /// installed pack has no wordmark face.
    Wordmark,
}
