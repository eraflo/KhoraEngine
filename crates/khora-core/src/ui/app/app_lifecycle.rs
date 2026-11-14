// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! The `AppLifecycle` trait: optional startup and shutdown hooks for an app.

use super::AppContext;

/// Optional lifecycle hooks — implement on the same type as [`App`](super::App)
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
