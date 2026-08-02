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

use khora_core::script::ScriptValue;
use khora_macros::Component;
use serde::{Deserialize, Serialize};

/// The behavior an entity runs, and the state that behavior keeps.
///
/// Attaching one is how an entity gets gameplay logic. Several may sit on one
/// entity: composition replaces inheritance here, which is how the ECS already
/// models the world — a second hierarchy inside the language would work against
/// it.
///
/// # Why the fields are values, not a compiled state blob
///
/// [`fields`](Self::fields) holds what a designer authored in the inspector
/// *and* what the behavior has since accumulated, in one name-keyed list. Two
/// things fall out of that shape rather than needing machinery:
///
/// - **Hot-reload keeps what still makes sense.** A script that gains or loses a
///   field is matched by name, so the fields that survived the edit keep their
///   values instead of the instance starting over.
/// - **The inspector needs no script-specific code.** A `ScriptValue` is the
///   same thing a component write carries, so the editor renders and edits these
///   the way it renders anything else.
///
/// The *running* VM state — program counter, registers, a suspended
/// continuation — is deliberately not here. It belongs to the script lane, which
/// is the only thing that may mutate it during a frame; it is written back here
/// when the scene is saved.
#[derive(Debug, Clone, PartialEq, Component, Default, Serialize, Deserialize)]
#[component(domain = Script)]
pub struct Script {
    /// The module the behavior is declared in, relative to the script root —
    /// `"ai/guard.erg"`.
    pub module: String,

    /// The behavior's name within that module — `"Guard"`.
    ///
    /// Named separately rather than folded into a `"path::Name"` string so a
    /// module rename and a behavior rename are two different edits, and neither
    /// has to parse the other out.
    pub behavior: String,

    /// The behavior's fields, by name.
    pub fields: Vec<(String, ScriptValue)>,
}

impl Script {
    /// A behavior with no field values set, so every field takes its declared
    /// default.
    pub fn new(module: impl Into<String>, behavior: impl Into<String>) -> Self {
        Self {
            module: module.into(),
            behavior: behavior.into(),
            fields: Vec::new(),
        }
    }

    /// Sets a field, replacing any value already under that name.
    pub fn with_field(mut self, name: impl Into<String>, value: ScriptValue) -> Self {
        let name = name.into();
        match self
            .fields
            .iter_mut()
            .find(|(existing, _)| *existing == name)
        {
            Some((_, slot)) => *slot = value,
            None => self.fields.push((name, value)),
        }
        self
    }

    /// Reads a field by name.
    pub fn field(&self, name: &str) -> Option<&ScriptValue> {
        self.fields
            .iter()
            .find(|(existing, _)| existing == name)
            .map(|(_, value)| value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_reads_back_by_name() {
        let script = Script::new("ai/guard.erg", "Guard")
            .with_field("speed", ScriptValue::Float(3.0))
            .with_field("health", ScriptValue::Int(100));

        assert_eq!(script.field("speed"), Some(&ScriptValue::Float(3.0)));
        assert_eq!(script.field("health"), Some(&ScriptValue::Int(100)));
        assert_eq!(script.field("missing"), None);
    }

    /// Setting a field twice replaces it. Appending instead would leave the
    /// inspector showing one value and the behavior reading another.
    #[test]
    fn setting_a_field_twice_replaces_it() {
        let script = Script::new("ai/guard.erg", "Guard")
            .with_field("speed", ScriptValue::Float(3.0))
            .with_field("speed", ScriptValue::Float(5.0));

        assert_eq!(script.fields.len(), 1);
        assert_eq!(script.field("speed"), Some(&ScriptValue::Float(5.0)));
    }

    /// The fields travel in the scene file, so they round-trip — this is what
    /// makes a designer's inspector edit survive a save.
    #[test]
    fn a_script_survives_serialization() {
        let script = Script::new("ai/guard.erg", "Guard")
            .with_field("speed", ScriptValue::Float(3.0))
            .with_field("waypoints", ScriptValue::Array(vec![ScriptValue::Int(1)]));

        let json = serde_json::to_string(&script).expect("serialises");
        let revived: Script = serde_json::from_str(&json).expect("deserialises");

        assert_eq!(revived, script);
    }
}
