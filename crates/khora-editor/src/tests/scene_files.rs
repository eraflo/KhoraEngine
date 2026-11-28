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

//! Scene files written for the editor's tests, wherever a test needs one.

use std::path::Path;

use khora_sdk::{GameWorld, SerializationGoal, SerializationService};

/// Writes `world` as a `.kscene` at `path`, as the editor saves a scene, for a test to open; panics when it cannot.
pub(crate) fn write_scene(world: &GameWorld, path: &Path) {
    let file = SerializationService::new()
        .save_world(world.inner_world(), SerializationGoal::EditorInterchange)
        .expect("the scene serializes");
    std::fs::write(path, file.to_bytes()).expect("the scene file is written");
}
