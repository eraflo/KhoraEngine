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

//! Whether a UI theme is dark or light.

/// Whether a [`crate::ui::UiTheme`] is a dark or a light theme.
///
/// UI backends pick their base visuals from it — a light palette laid over a
/// backend's dark defaults leaves dark leftovers in every stock widget the
/// palette does not name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorScheme {
    /// Light ink on a dark field.
    #[default]
    Dark,
    /// Dark ink on a light field.
    Light,
}
