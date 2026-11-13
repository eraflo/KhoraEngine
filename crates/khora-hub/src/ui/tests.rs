// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

use super::*;
use khora_sdk::tool_ui::ColorScheme;

/// The hub is the editor's negative: a light field, where the editor
/// stays dark.
#[test]
fn hub_theme_is_light() {
    assert_eq!(theme().scheme, ColorScheme::Light);
}
