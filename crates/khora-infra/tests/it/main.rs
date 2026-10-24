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

//! khora-infra integration tests, compiled as one test binary.
//!
//! Each file under `tests/` is its own executable, and each executable is a
//! full link of the crate's dependency tree. New integration tests join this
//! binary as a module instead of adding a file at the root of `tests/`.

mod bounded_drag_value;
mod cover_panel;
mod covered_hover;
mod dnd_hover_payload;
mod gradient_fill;
mod light_culling_layout;
mod menu_on_click;
mod public_paths;
mod rapier_attached_colliders;
mod rapier_body_rotation;
mod rapier_sensor_pairs;
mod rapier_sensors;
mod rapier_shape_updates;
mod wordmark_font_family;
