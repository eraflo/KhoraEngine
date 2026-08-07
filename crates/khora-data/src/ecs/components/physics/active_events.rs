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

use khora_macros::Component;
use serde::{Deserialize, Serialize};

/// Indicates that an entity should receive collision events.
/// By default, the physics engine does not report collisions for every entity
/// to save on performance. Adding this component enables reporting for this entity.
// `Authored`, the default, and deliberately: a designer adds this to say "report
// this collider's contacts". It was `#[component(no_serializable)]`, which does
// not mean "runtime state" — it removes the `ComponentRegistration` entirely, so
// no serializer ever knew the type existed. Saving a scene and loading it back
// silently dropped it, and the collider went quiet.
#[derive(Debug, Clone, Copy, Default, Component, Serialize, Deserialize)]
#[component(domain = Physics)]
pub struct ActiveEvents;
