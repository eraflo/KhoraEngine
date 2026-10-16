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
/// lifecycle. The snapshot's `fields` are the behavior's component's, and are
/// left out.
pub fn companion_row(
    snapshot: &ScriptSnapshot,
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
        (
            "spawned".to_owned(),
            ScriptValue::Bool(snapshot.lifecycle.spawned),
        ),
        ("fault".to_owned(), to_value(&snapshot.lifecycle.fault)?),
        (
            "resume_failed".to_owned(),
            to_value(&snapshot.lifecycle.resume_failed)?,
        ),
        (
            "owed".to_owned(),
            ScriptValue::Float(snapshot.lifecycle.owed),
        ),
        (
            "fixed_owed".to_owned(),
            ScriptValue::Array(
                snapshot
                    .lifecycle
                    .fixed_owed
                    .iter()
                    .map(|step| ScriptValue::Float(*step))
                    .collect(),
            ),
        ),
    ])
}

/// What a companion's fields record: the snapshot, its own fields empty.
/// A field the companion no longer declares — `initialised`, from before an
/// instance's defaults were data — is ignored.
pub fn companion_snapshot(
    row: &[(String, ScriptValue)],
) -> Result<ScriptSnapshot, ValueSerdeError> {
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
            // A companion from before owed time was kept owes none.
            owed: match row.iter().find(|(known, _)| known == "owed") {
                Some((_, ScriptValue::Float(owed))) => *owed,
                _ => 0.0,
            },
            fixed_owed: match row.iter().find(|(known, _)| known == "fixed_owed") {
                Some((_, ScriptValue::Array(steps))) => steps
                    .iter()
                    .filter_map(|step| match step {
                        ScriptValue::Float(step) if *step > 0.0 => Some(*step),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            },
        },
        ..ScriptSnapshot::default()
    };
    Ok(snapshot)
}
