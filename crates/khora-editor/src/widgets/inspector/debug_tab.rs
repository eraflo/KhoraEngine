// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! `DebugTab`: the Inspector tab that shows the undo / redo stack.

use khora_sdk::editor_ui::UiBuilder;

use super::tabs::{InspectorTab, InspectorTabContext};

/// Debug tab — undo / redo stack inspection.
pub struct DebugTab;

impl InspectorTab for DebugTab {
    fn label(&self) -> &str {
        "Debug"
    }

    fn render(
        &mut self,
        ui: &mut dyn UiBuilder,
        body_rect: [f32; 4],
        ctx: &mut InspectorTabContext<'_>,
    ) {
        let theme = ctx.theme.clone();
        let history = ctx.history.clone();
        let undo_desc = history
            .lock()
            .ok()
            .and_then(|h| h.undo_description().map(|s| s.to_owned()))
            .unwrap_or_else(|| "(none)".to_owned());
        let redo_desc = history
            .lock()
            .ok()
            .and_then(|h| h.redo_description().map(|s| s.to_owned()))
            .unwrap_or_else(|| "(none)".to_owned());
        ui.region_at("inspector-debug", body_rect, &mut |ui_inner| {
            ui_inner.colored_label(theme.text_dim, "Command history:");
            ui_inner.colored_label(
                theme.text_muted,
                &format!("Undo: {} | Redo: {}", undo_desc, redo_desc),
            );
        });
    }
}
