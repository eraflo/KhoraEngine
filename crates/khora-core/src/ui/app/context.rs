// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! `AppContext` — per-frame handle handed to [`super::App::update`].
//!
//! Wraps the backend's per-tick context (egui's `Context`, in
//! practice) behind a neutral surface. Apps use it to:
//!
//! - install / swap the [`UiTheme`] and [`FontPack`] at startup;
//! - request a repaint from a background thread or async event;
//! - query the screen size for responsive layouts;
//! - hand a callback the central paint region (which receives a
//!   `&mut dyn UiBuilder` — same trait the editor's panels use).

use crate::ui::editor::UiBuilder;
use crate::ui::{FontPack, UiTheme};

/// Top-level app context — one per frame.
pub trait AppContext {
    /// Open the central drawing region. The closure receives a
    /// `&mut dyn UiBuilder` and lays out the frame's content.
    ///
    /// Equivalent to "the whole window minus any panels installed at
    /// the backend level". For tools without OS-level panels (the
    /// hub) this is the only call needed per frame.
    fn central(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder));

    /// Apply a [`UiTheme`] to the underlying backend's visual
    /// configuration. Idempotent — call again to swap palettes at
    /// runtime.
    fn set_theme(&mut self, theme: &UiTheme);

    /// Install the fonts described by `pack`. Called once at startup
    /// (or whenever fonts change).
    fn set_fonts(&mut self, pack: &FontPack);

    /// Logical screen size in pixels — `[width, height]`.
    fn screen_size(&self) -> [f32; 2];

    /// Pixels-per-point ratio (DPI scale). 1.0 on standard displays,
    /// 2.0 on Retina, etc.
    fn pixels_per_point(&self) -> f32 {
        1.0
    }

    /// Ask the backend for another paint pass on the next available
    /// frame, even if no input event would normally trigger one. Use
    /// when an off-thread async task completes and the visible UI
    /// must update.
    fn request_repaint(&mut self);

    /// Request the application window to close. The shutdown happens
    /// at the end of the current frame (or whenever the backend gets
    /// to it).
    fn request_close(&mut self) {
        // Default no-op — backends that support it override.
    }
}
