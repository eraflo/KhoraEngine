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

//! khora-lanes integration tests, compiled as one test binary.
//!
//! Each file under `tests/` is its own executable, and each executable is a
//! full link of the crate's dependency tree. New integration tests join this
//! binary as a module instead of adding a file at the root of `tests/`.

mod array_fields;
mod authored_base;
mod durable_text;
mod frozen_save;
mod held_text;
mod inventory_fields;
mod lifecycle_restore;
mod nested_array_saves;
mod null_fields;
mod null_into_declared_types;
mod on_resume_failed;
mod overdraft;
mod public_paths;
mod resumable_after_edits;
mod resumable_edges;
mod resume_tiers;
mod resume_tiers_encodings;
mod saved_structs_into_edited_types;
mod saves;
mod spawn_scenarios;
mod starved_turn;
mod state_entry_across_edits;
mod struct_fields;
