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

//! The events the engine itself raises for scripts, declared as a script
//! declares an `event`.

/// Something started touching the entity: carries the other one.
pub const TOUCHED: EngineEvent = EngineEvent {
    name: "Touched",
    params: &[("Entity", "other")],
};

/// Something stopped touching the entity: carries the other one.
pub const SEPARATED: EngineEvent = EngineEvent {
    name: "Separated",
    params: &[("Entity", "other")],
};

/// Every event the engine raises — what scripts handle without declaring.
pub const ENGINE_EVENTS: &[EngineEvent] = &[TOUCHED, SEPARATED];

/// An event the engine raises, declared for scripts as an `event` is: its
/// name and what it carries — the same signature a script's raise is held to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineEvent {
    /// The name a handler spells.
    pub name: &'static str,
    /// What it carries, in order: each parameter's Ergon type, spelled as a
    /// script writes it, and its name.
    pub params: &'static [(&'static str, &'static str)],
}
