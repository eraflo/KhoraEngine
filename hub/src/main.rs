// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Khora Engine Hub — standalone project manager and engine launcher.
//!
//! Reaches the egui+eframe backend exclusively through
//! `khora_sdk::tool_ui::*` — never imports `egui` or `eframe` directly.
//!
//! Module map:
//!
//! | Module           | Responsibility                                   |
//! |------------------|--------------------------------------------------|
//! | `app`            | `HubApp` struct + non-rendering domain methods   |
//! | `state`          | One state struct per screen + `Banner`/`Screen`  |
//! | `chrome`         | Top bar, status bar, banner overlay              |
//! | `screens`        | Per-screen UI (home, settings, …)                |
//! | `services`       | Hub services (config, download, git, github, …)  |
//! | `ui`             | Theme, fonts, widgets                            |
//! | `async_pump`     | Per-frame drain of background channels           |
//! | `bootstrap`      | `App::update` impl + `run()` entry point         |

mod app;
mod async_pump;
mod bootstrap;
mod chrome;
mod screens;
mod services;
mod state;
mod ui;

// Re-exports keep the old `crate::xxx` paths working.
pub use app::HubApp;
pub use services::config;
pub use services::download;
pub use services::git;
pub use services::github;
pub use services::github::auth;
pub use services::project;
pub use state::{
    AuthState, Banner, EngineChoice, EngineManagerState, HomeState, NewProjectState, Screen,
    SettingsState,
};
pub use ui::fonts;
pub use ui::theme;
pub use ui::widgets;

fn main() -> anyhow::Result<()> {
    bootstrap::run()
}
