// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! The transient banner — a toast pinned under the top bar.

use crate::Banner;
use khora_sdk::tool_ui::UiBuilder;
use khora_tool_ui::brand::khora_dark;
use khora_tool_ui::widgets::{Tone, banner as toast};

/// Paints the banner. Returns `true` if the user dismissed it.
pub fn paint_banner(ui: &mut dyn UiBuilder, banner: &Banner) -> bool {
    let t = khora_dark();
    let r = ui.panel_rect();

    let w = (r[2] - 80.0).min(460.0);
    let h = 56.0;
    let rect = [r[0] + (r[2] - w) * 0.5, r[1] + 16.0, w, h];

    let (title, tone) = if banner.is_error {
        ("Something went wrong", Tone::Error)
    } else {
        ("Done", Tone::Info)
    };

    toast(ui, &t, rect, "hub-banner", title, &banner.message, tone).clicked
}
