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

use khora_core::script::ScriptSnapshot;
use khora_macros::Component;
use serde::{Deserialize, Serialize};

/// What the script lane observed of an entity's behavior: its fields as they
/// are now, its state, its countdowns, a sequence stopped at an `await`.
///
/// Written by the engine, never by an author — a scene never holds one; a game
/// save does, so a guard saved mid-attack loads mid-attack.
#[derive(Debug, Clone, PartialEq, Component, Default, Serialize, Deserialize)]
#[component(domain = Script, provenance = Runtime, resumable)]
pub struct ScriptState {
    /// The behavior that observed it. A state recorded for another behavior
    /// is never handed to this one.
    pub behavior: String,
    /// What it observed.
    pub snapshot: ScriptSnapshot,
}
