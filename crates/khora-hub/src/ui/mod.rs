// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Hub UI infrastructure.
//!
//! The look — brand palette, widget vocabulary — comes from the
//! `khora-tool-ui` crate, shared with the editor so the two tools cannot
//! drift. Nothing visual is defined here.
//!
//! What remains is hub-specific: loading the font files, and the handful of
//! helpers that format the hub's own data.

pub mod format;

use khora_sdk::tool_ui::UiTheme;

/// The hub's theme — the one switch every hub surface reads its look from.
///
/// The hub is light silver: the negative of the editor, which stays in the
/// deep field, as the hub's icon is the negative of the editor's.
pub fn theme() -> UiTheme {
    khora_tool_ui::brand::khora_light()
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_sdk::tool_ui::ColorScheme;

    /// The hub is the editor's negative: a light field, where the editor
    /// stays dark.
    #[test]
    fn hub_theme_is_light() {
        assert_eq!(theme().scheme, ColorScheme::Light);
    }
}
