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

mod camera_aspect_follows_target;
mod camera_view_prepares_the_frame;
mod declared_components;
mod editor_shader_sources;
mod engine_mode;
mod engine_names;
mod frame_target_reaches_lanes;
mod game_saves;
mod lane_agents_at_boot;
mod mode_to_dcc;
mod overlay_host_views;
mod pass_order;
mod physics_step_setting;
mod prefab_library;
mod problems_without_scripts;
mod public_paths;
mod reload_after_an_emptied_module;
mod removed_phases;
mod render_doubles;
mod runtime_config_file;
mod script_catalog;
mod script_problems;
mod script_problems_after_remount;
mod script_problems_after_unmount;
mod scripts_mount_sequences;
mod scripts_mounted_while_running;
mod scripts_outside_the_script_folder;
mod scripts_unmounted_after_play;
mod staged_bootstrap;
mod system_components;
