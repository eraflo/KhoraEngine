// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! The banner and the refusals listed — toasts pinned under the top bar,
//! one under the other.

use crate::state::{Banner, ProjectRefusal};
use khora_sdk::tool_ui::UiBuilder;
use khora_tool_ui::widgets::{Tone, banner as toast};

/// The height of one toast.
const TOAST_HEIGHT: f32 = 56.0;
/// The space between two toasts, and above the first.
const TOAST_GAP: f32 = 16.0;

/// Paints the banner in the first slot. Returns `true` if the user dismissed it.
pub(crate) fn paint_banner(ui: &mut dyn UiBuilder, banner: &Banner) -> bool {
    let (title, tone) = if banner.is_error {
        ("Something went wrong", Tone::Error)
    } else {
        ("Done", Tone::Info)
    };
    paint_toast(ui, 0, "hub-banner", title, &banner.message, tone)
}

/// Paints `refusal`, a refusal no banner shows, in slot `slot` (0 is the first). Returns `true` if the user dismissed it.
pub(crate) fn paint_refusal(ui: &mut dyn UiBuilder, refusal: &ProjectRefusal, slot: usize) -> bool {
    let id = format!("hub-refusal-{slot}");
    paint_toast(ui, slot, &id, "Refused", &refusal.shown(), Tone::Error)
}

/// Paints a toast in slot `slot`, under the top bar. Returns `true` if the user dismissed it.
fn paint_toast(
    ui: &mut dyn UiBuilder,
    slot: usize,
    id: &str,
    title: &str,
    message: &str,
    tone: Tone,
) -> bool {
    let t = crate::ui::theme();
    let r = ui.panel_rect();
    let w = (r[2] - 80.0).min(460.0);
    let y = r[1] + TOAST_GAP + slot as f32 * (TOAST_HEIGHT + TOAST_GAP / 2.0);
    let rect = [r[0] + (r[2] - w) * 0.5, y, w, TOAST_HEIGHT];
    toast(ui, &t, rect, id, title, message, tone).clicked
}
