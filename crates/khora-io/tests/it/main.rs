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

//! khora-io integration tests, compiled as one test binary.
//!
//! Each file under `tests/` is its own executable, and each executable is a
//! full link of the crate's dependency tree. New integration tests join this
//! binary as a module instead of adding a file at the root of `tests/`.

mod asset_watch;
mod asset_watch_repeated_saves;
mod asset_watch_root_spellings;
mod behavior_components;
mod compile_problems;
mod compile_problems_found_by_importers;
mod compile_problems_located_in_the_engine_mirror;
mod compile_problems_of_importers_by_any_spelling;
mod declared_asset_fields;
mod declared_component_scenes;
mod declared_field_hints;
mod declared_names_in_the_catalog;
mod game_saves;
mod game_saves_text_base;
mod hot_reload_announcements;
mod hot_reload_from_the_shared_watch;
mod hot_reload_of_misplaced_scripts;
mod hot_reload_of_removed_scripts;
mod prefab_assets;
mod prefab_scenes;
mod public_paths;
mod registration_problems;
mod registration_problems_of_an_inherited_name;
mod registration_problems_of_every_refused_module;
mod scene_files;
mod script_components;
mod script_pump_across_roots;
mod shipped_scripts;
mod snapshots;
mod system_components;
mod unit_typed_declarations;
