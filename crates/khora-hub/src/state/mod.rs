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
mod card_fact;
mod card_facts;
mod engine_choice;
pub mod engine_manager;
pub mod home;
pub mod new_project;
mod project_refusal;
mod project_refusals;
pub mod screen;
pub mod settings;

pub use auth::AuthState;
pub use banner::Banner;
pub use card_fact::CardFact;
pub use card_facts::CardFacts;
pub use engine_manager::EngineManagerState;
pub use home::HomeState;
pub use new_project::{EngineChoice, NewProjectState};
pub use project_refusal::ProjectRefusal;
pub use project_refusals::ProjectRefusals;
pub use screen::Screen;
pub use settings::SettingsState;
