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

//! khora-sdk integration tests, compiled as one test binary.
//!
//! Each file under `tests/` is its own executable, and each executable is a
//! full link of the crate's dependency tree. New integration tests join this
//! binary as a module instead of adding a file at the root of `tests/`.

mod activation;
mod app_provides_to_the_runtime;
mod assets_decoded_off_the_frame;
mod camera_aspect_follows_target;
mod camera_view_prepares_the_frame;
mod declared_components;
mod editor_shader_sources;
mod engine_names;
mod frame_boundary;
mod frame_report;
mod frame_target_reaches_lanes;
mod game_saves;
mod host_monitors;
mod lane_agents_at_boot;
mod overlay_families;
mod overlay_host_views;
mod pass_order;
mod physics_step_setting;
mod prefab_library;
mod problems_without_scripts;
mod project_indexed_off_the_frame;
mod protocol;
mod public_paths;
mod quotes_follow_the_target;
mod reload_after_an_emptied_module;
mod removed_phases;
mod render_doubles;
mod render_family_at_boot;
mod runtime_config_file;
mod scene_loads_off_the_frame;
mod script_catalog;
mod script_problems;
mod script_problems_after_remount;
mod script_problems_after_unmount;
mod scripts_compiled_off_the_frame;
mod scripts_mount_sequences;
mod scripts_mounted_by_ticket;
mod scripts_mounted_while_running;
mod scripts_outside_the_script_folder;
mod scripts_scoped_to_a_marker;
mod scripts_switched_on_in_before_frame;
mod scripts_unmounted_after_play;
mod staged_bootstrap;
mod startup_readiness;
mod system_components;
mod ui_family;
