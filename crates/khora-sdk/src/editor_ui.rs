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

//! Editor UI types re-exported from khora_core.
//!
//! Includes everything from `khora_core::ui::editor::*` plus the
//! shared `UiTheme` and font types that live one level up in
//! `khora_core::ui` (because the hub uses them too).
pub use khora_core::ui::editor::*;
pub use khora_core::ui::fonts::{FontHandle, FontPack, NamedFont};
pub use khora_core::ui::theme::UiTheme;
