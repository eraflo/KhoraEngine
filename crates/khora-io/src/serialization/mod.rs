// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Scene serialization — on-demand service backed by strategies from khora-data.
//!
//! The `SerializationStrategy` trait and concrete implementations live in
//! `khora_data::scene`. This module provides the `SerializationService` that
//! orchestrates them for file I/O.

mod service;

// Re-export strategies from khora-data for convenience.
pub use khora_data::scene::{
    migrate_payload, ArchetypeSerializationStrategy, DefinitionSerializationStrategy,
    DeserializationError, MessagePackSerializationStrategy, MigrationError,
    RecipeSerializationStrategy, SceneMigration, SceneMigrationRegistration, SerializationError,
    SerializationStrategy,
};
pub use service::*;
