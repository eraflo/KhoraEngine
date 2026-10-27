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
mod behavior_budgets;
mod behavior_cursors;
mod behavior_fuel_shares;
mod critical_behaviors;
mod declared_behaviors;
mod durable_text;
mod faulted_turns;
mod fixed_update_owed;
mod fixed_update_turns;
mod frozen_save;
mod held_text;
mod hot_reload_in_step;
mod inventory_fields;
mod lifecycle_restore;
mod mail_across_a_play_restart;
mod nested_array_saves;
mod null_fields;
mod null_into_declared_types;
mod on_resume_failed;
mod overdraft;
mod public_paths;
mod rate_limited_behaviors;
mod resumable_after_edits;
mod resumable_edges;
mod resume_tiers;
mod resume_tiers_encodings;
mod running_problems;
mod saved_structs_into_edited_types;
mod saves;
mod scheduled_scene;
mod spatial_mixing;
mod spawn_scenarios;
mod starved_turn;
mod state_entry_across_edits;
mod struct_fields;
mod watchdog;
mod watchdog_across_saves;
mod watchdog_debt;
mod watchdog_debt_across_edits_and_loads;
mod watchdog_debt_of_a_finished_body;
mod watchdog_first_frame;
mod watchdog_on_spawn_before_a_restored_body;
mod watchdog_restored;
mod watchdog_stage;
mod watchdog_timers;
mod withdrawn_modules;
mod writes_earlier_in_the_frame;
