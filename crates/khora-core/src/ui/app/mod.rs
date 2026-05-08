// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Backend-agnostic app runtime — `App` trait + `AppContext` trait.
//!
//! Standalone tools (the hub, future asset cooker, …) implement [`App`]
//! and receive a [`AppContext`] each frame. Neither the trait nor its
//! impls expose `egui` / `eframe` types — the concrete backend lives
//! in `khora-infra::ui::egui::app`.
//!
//! Apps don't import this module directly; they reach it through the
//! `khora-sdk` re-exports + a `run_native()` function that boots the
//! eframe backend behind the trait.

pub mod context;
pub mod runtime;

pub use context::AppContext;
pub use runtime::{App, AppLifecycle};
