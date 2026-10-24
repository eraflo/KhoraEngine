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

//! The names a script cannot declare by, as a tool asks before offering one:
//! the engine's components, the language's asset kinds and the events the
//! engine raises.

use khora_sdk::scripts::is_engine_name;

#[test]
fn an_engine_components_name_is_the_engines() {
    for name in ["Transform", "Camera", "Light", "RigidBody", "AudioSource"] {
        assert!(is_engine_name(name), "`{name}`");
    }
}

#[test]
fn an_asset_kinds_name_is_the_engines() {
    for name in ["Prefab", "Sound", "Material", "Mesh", "Texture", "Scene"] {
        assert!(is_engine_name(name), "`{name}`");
    }
}

/// **An event the engine raises is in every module's scope**, so a script
/// declaring a behavior by its name does not compile.
#[test]
fn an_engine_events_name_is_the_engines() {
    for event in khora_sdk::khora_core::script::ENGINE_EVENTS {
        assert!(is_engine_name(event.name), "`{}`", event.name);
    }
    assert!(is_engine_name("Touched") && is_engine_name("Separated"));
}

#[test]
fn a_name_the_engine_does_not_use_is_free_and_case_counts() {
    for name in ["Player", "Mover", "camera", "TRANSFORM", ""] {
        assert!(!is_engine_name(name), "`{name}`");
    }
}
