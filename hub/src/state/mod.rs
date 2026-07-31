// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Hub state — one module per screen plus the shared `Banner` and
//! `Screen` enum.

pub mod auth;
pub mod banner;
pub mod engine_manager;
pub mod home;
pub mod new_project;
pub mod screen;

pub use auth::{AuthState, SettingsState};
pub use banner::Banner;
pub use engine_manager::EngineManagerState;
pub use home::HomeState;
pub use new_project::{EngineChoice, NewProjectState};
pub use screen::Screen;
