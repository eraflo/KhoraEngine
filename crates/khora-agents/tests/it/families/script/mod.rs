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

//! The script family, registered and booted as the engine boots it: one
//! `LaneAgent` built from the registration the family submitted, checked
//! against the runtime before the first frame.
//!
//! These replace the hand-written script agent's own tests: what it declared,
//! that it ran from the first frame its family was on, that a deferral kept
//! the mail, and what it reported.

mod behavior_budget;
mod behavior_game_saves;
mod behavior_hot_reload;
mod behavior_removal;
mod behavior_session;
mod behavior_write_order;
mod behavior_writeback;
mod behaviors_as_components;
mod behaviors_on_one_entity;
mod collision_to_behavior_test;
mod epochs;
mod faulted_turns_in_play;
pub mod fixtures;
mod imported_systems;
mod input_from_script_test;
mod prefabs_spawned_in_play;
mod registration;
mod runs;
mod save_mid_attack_test;
mod scene_edit_after_save;
mod scheduled_behaviors_in_negotiation;
mod script_camera_writes;
mod script_component_reads;
mod script_component_writes;
mod script_defined_components;
mod script_delivery_test;
mod script_fuel_each_frame;
mod script_intake;
mod script_offer_rows;
mod shipped_script_runs_test;
mod spawned_in_play;
mod staged_reloads;
mod system_budget;
mod system_kind_changes;
mod system_kind_round_trips;
mod system_ref_writes;
mod system_row_order;
mod system_rows_across_components;
mod system_session;
mod system_singleton_encodings;
mod system_singletons;
mod system_sweeps_across_an_edit;
mod systems_run;
mod unlent_runs;
mod watchdog_in_play;
