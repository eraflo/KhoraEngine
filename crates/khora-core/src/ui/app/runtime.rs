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

/// One application — implements `update` and gets called once per
/// frame.
///
/// Tools should be single types (a struct holding all the app state)
/// implementing this trait. The boot helper instantiates the type
/// once, then drives `update` for the lifetime of the window.
pub trait App {
    /// Per-frame update — paint UI, react to input, dispatch async
    /// completions, etc.
    fn update(&mut self, ctx: &mut dyn AppContext);

    /// Called once after the backend is up and before the first
    /// `update`. Apps install their theme / fonts / DPI snapping
    /// here. Default is a no-op.
    fn on_start(&mut self, ctx: &mut dyn AppContext) {
        let _ = ctx;
    }

    /// Called once when the window is closing. Persist state here.
    /// Default is a no-op.
    fn on_exit(&mut self) {}
}

/// Optional lifecycle hooks — implement on the same type as [`App`]
/// to hook startup / shutdown without inflating the main trait.
pub trait AppLifecycle {
    /// Called once after the backend is up but before the first
    /// `update`. Apps install their theme / fonts here.
    fn on_start(&mut self, ctx: &mut dyn AppContext) {
        let _ = ctx;
    }

    /// Called once when the window is closing. Persist state here.
    fn on_exit(&mut self) {}
}

// Default `AppLifecycle` impl so apps that don't need it don't have
// to declare anything extra — the boot helper detects whether the
// concrete type opted in via a separate trait bound.
impl<T: ?Sized> AppLifecycle for T {}
