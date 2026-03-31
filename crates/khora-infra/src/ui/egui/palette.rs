// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Editor egui palette — `Color32` constants matching the Khora Hub brand.
//!
//! This is a deliberate duplication of the hub palette so that `khora-infra`
//! does not depend on the standalone hub crate.

#![allow(missing_docs)]

use egui::Color32;

pub const BG: Color32 = Color32::from_rgb(10, 10, 14);
pub const TOOLBAR_BG: Color32 = Color32::from_rgb(10, 11, 15);
pub const STATUS_BAR_BG: Color32 = Color32::from_rgb(10, 10, 14);
pub const SURFACE: Color32 = Color32::from_rgb(17, 18, 23);
pub const SURFACE2: Color32 = Color32::from_rgb(24, 26, 33);
pub const TAB_BAR_BG: Color32 = Color32::from_rgb(20, 22, 28);
pub const SURFACE3: Color32 = Color32::from_rgb(34, 37, 48);
pub const BORDER: Color32 = Color32::from_rgb(38, 42, 56);
pub const BORDER_LIGHT: Color32 = Color32::from_rgb(55, 60, 78);
pub const PRIMARY: Color32 = Color32::from_rgb(58, 135, 240);
pub const PRIMARY_DIM: Color32 = Color32::from_rgb(25, 65, 130);
pub const PRIMARY_BORDER: Color32 = Color32::from_rgb(38, 90, 190);
pub const MODE_BG: Color32 = Color32::from_rgb(18, 40, 80);
pub const ACCENT: Color32 = Color32::from_rgb(124, 92, 222);
pub const TEXT: Color32 = Color32::from_rgb(226, 232, 240);
pub const TEXT_BRIGHT: Color32 = Color32::from_rgb(200, 210, 228);
pub const TEXT_DIM: Color32 = Color32::from_rgb(136, 146, 164);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(80, 88, 106);
pub const HINT_TEXT: Color32 = Color32::from_rgb(70, 78, 96);
pub const SUCCESS: Color32 = Color32::from_rgb(58, 184, 122);
pub const PLAY_GREEN: Color32 = Color32::from_rgb(100, 220, 100);
pub const FPS_GREEN: Color32 = Color32::from_rgb(100, 180, 100);
pub const WARNING: Color32 = Color32::from_rgb(240, 160, 58);
pub const ERROR: Color32 = Color32::from_rgb(240, 90, 58);
pub const STOP_RED: Color32 = Color32::from_rgb(220, 80, 80);
pub const DISABLED: Color32 = Color32::from_rgb(120, 120, 120);
