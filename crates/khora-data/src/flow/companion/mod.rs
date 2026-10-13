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

//! A behavior's state companion, read as a [`ScriptSnapshot`] and written
//! from one.
//!
//! The companion `Guard#state` holds what the engine keeps of a running
//! instance — its state by name, that state's data, its countdowns, a
//! suspended sequence, its lifecycle — in the same fixed fields for every
//! behavior. The behavior's own fields are not part of it: they are the
//! behavior's component, `Guard`.

use khora_core::script::value_serde::{from_value, to_value, ValueSerdeError};
use khora_core::script::{InstanceLifecycle, ScriptSnapshot, ScriptValue};

#[cfg(test)]
mod tests;

/// The companion's fields, by name, in its layout's order, recording
/// `snapshot`'s state, state data, countdowns, suspended sequence and
/// lifecycle, and whether the instance's initialiser has run. The snapshot's
/// `fields` are the behavior's component's, and are left out.
pub fn companion_row(
    snapshot: &ScriptSnapshot,
    initialised: bool,
) -> Result<Vec<(String, ScriptValue)>, ValueSerdeError> {
    let text = |value: &Option<String>| value.clone().map_or(ScriptValue::Null, ScriptValue::Str);
    Ok(vec![
        ("state".to_owned(), text(&snapshot.state)),
        (
            "state_fields".to_owned(),
            ScriptValue::Struct(snapshot.state_fields.clone()),
        ),
        ("timers".to_owned(), to_value(&snapshot.timers)?),
        ("pending".to_owned(), to_value(&snapshot.pending)?),
        ("initialised".to_owned(), ScriptValue::Bool(initialised)),
        (
            "spawned".to_owned(),
            ScriptValue::Bool(snapshot.lifecycle.spawned),
        ),
        ("fault".to_owned(), to_value(&snapshot.lifecycle.fault)?),
        (
            "resume_failed".to_owned(),
            to_value(&snapshot.lifecycle.resume_failed)?,
        ),
    ])
}

/// What a companion's fields record: the snapshot, its own fields empty,
/// and whether the instance's initialiser has run.
pub fn companion_snapshot(
    row: &[(String, ScriptValue)],
) -> Result<(ScriptSnapshot, bool), ValueSerdeError> {
    let field = |name: &str| {
        row.iter()
            .find(|(known, _)| known == name)
            .map(|(_, value)| value)
            .ok_or_else(|| ValueSerdeError(format!("a state companion with no `{name}`")))
    };
    let flag = |name: &str| match field(name)? {
        ScriptValue::Bool(value) => Ok(*value),
        other => Err(ValueSerdeError(format!(
            "`{name}` is a flag, not {}",
            other.type_name()
        ))),
    };
    let state = match field("state")? {
        ScriptValue::Null => None,
        ScriptValue::Str(name) => Some(name.clone()),
        other => {
            return Err(ValueSerdeError(format!(
                "`state` is a state's name, not {}",
                other.type_name()
            )))
        }
    };
    let state_fields = match field("state_fields")? {
        ScriptValue::Struct(fields) => fields.clone(),
        ScriptValue::Null => Vec::new(),
        other => {
            return Err(ValueSerdeError(format!(
                "`state_fields` is a state's data, not {}",
                other.type_name()
            )))
        }
    };
    let snapshot = ScriptSnapshot {
        state,
        state_fields,
        timers: from_value(field("timers")?)?,
        pending: from_value(field("pending")?)?,
        lifecycle: InstanceLifecycle {
            spawned: flag("spawned")?,
            fault: from_value(field("fault")?)?,
            resume_failed: from_value(field("resume_failed")?)?,
        },
        ..ScriptSnapshot::default()
    };
    Ok((snapshot, flag("initialised")?))
}
