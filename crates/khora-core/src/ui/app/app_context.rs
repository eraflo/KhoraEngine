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
    /// Opens the central drawing region, where `f` lays out the frame's content.
    ///
    /// The region is the whole window minus any panels installed at the
    /// backend level. A tool without such panels (the hub) needs only this call
    /// per frame.
    fn central(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder));

    /// Applies a [`UiTheme`] to the backend's visual configuration.
    ///
    /// Idempotent; call again to swap palettes at runtime.
    fn set_theme(&mut self, theme: &UiTheme);

    /// Installs the fonts described by `pack`, at startup or whenever fonts change.
    fn set_fonts(&mut self, pack: &FontPack);

    /// Returns the logical screen size in pixels, as `[width, height]`.
    fn screen_size(&self) -> [f32; 2];

    /// Returns the pixels-per-point ratio (DPI scale): 1.0 on standard displays, 2.0 on Retina.
    fn pixels_per_point(&self) -> f32 {
        1.0
    }

    /// Asks the backend for another paint pass, even with no input event.
    ///
    /// For when an off-thread task completes and the visible UI must update.
    fn request_repaint(&mut self);

    /// Asks the application window to close, at the end of the current frame at the earliest.
    ///
    /// Does nothing on a backend that cannot close its window.
    fn request_close(&mut self) {}
}
