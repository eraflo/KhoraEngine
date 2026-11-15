// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! `App` trait — what every standalone Khora tool implements.
//!
//! The trait is intentionally tiny: an `update` per frame plus
//! optional lifecycle hooks. The concrete `run_native` boot function
//! that drives an `App` impl lives in `khora-infra::ui::egui::app`
//! and is re-exported through `khora-sdk`.

use super::AppContext;

pub use super::app_lifecycle::AppLifecycle;

/// A standalone tool, updated once per frame for the lifetime of its window.
///
/// A tool is one type holding all its state; the boot helper builds it once.
pub trait App {
    /// Updates the tool for one frame: paints UI, reacts to input, handles async completions.
    fn update(&mut self, ctx: &mut dyn AppContext);

    /// Runs once after the backend is up and before the first `update`.
    ///
    /// The place to install the theme, fonts and DPI snapping. Does nothing by default.
    fn on_start(&mut self, ctx: &mut dyn AppContext) {
        let _ = ctx;
    }

    /// Runs once when the window is closing, the place to persist state.
    ///
    /// Does nothing by default.
    fn on_exit(&mut self) {}
}
