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

//! Everything a behavior is, written down.
//!
//! A guard saved mid-patrol should load mid-patrol. Its fields are the obvious
//! part, and the part that is easy to mistake for the whole: a behavior is also
//! *which state it is in*, that state's own data, how long its countdowns have
//! left, and — if it stopped at an `await` — where in the code it stopped.
//! Saving only the fields loads a guard that has forgotten it was chasing
//! anyone.
//!
//! # Named, never numbered
//!
//! Every part of this is keyed by something the **author wrote** — a state's
//! name, a field's name, a timer's declared interval. Never by a slot or an
//! index, because those are assigned by the compiler and move when the script is
//! edited. A scene is read once and has to survive whatever happened to the
//! script between the save and the load; matching by name is what makes an
//! edited script load an old save instead of corrupting it.
//!
//! # What a countdown holds
//!
//! Time *remaining*, not the moment it is due. An absolute deadline saved on
//! Tuesday and loaded on Friday either fires instantly or waits three days,
//! depending only on how the host measures time. What is left survives a save
//! exactly.

use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};

use super::ScriptValue;

/// One `every` or `after`, and how far along it is.
///
/// Identified by what the author wrote — `every 0.5s` — rather than by its
/// position among the behavior's members, so inserting a schedule above another
/// does not swap their countdowns. Two schedules an author wrote identically are
/// genuinely interchangeable, and [`ordinal`](Self::ordinal) tells those apart
/// only so a save with two of them restores two.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Encode, Decode)]
pub struct TimerRemaining {
    /// Whether it repeats — `every` rather than `after`.
    pub repeating: bool,
    /// The interval the author declared, in seconds.
    pub interval: f32,
    /// Which one, among schedules declared identically.
    pub ordinal: u32,
    /// Seconds still to wait.
    ///
    /// `None` for an `after` that has already fired: it is spent, not due in
    /// zero seconds, and the difference is whether loading the save makes it
    /// fire again.
    pub remaining: Option<f32>,
}

/// A member stopped part-way through an `await`.
///
/// The reason a continuation had to be *representable* rather than opaque, and
/// the promise the whole suspension design was built to keep: a save taken
/// half-way through an attack's wind-up loads half-way through it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Encode, Decode)]
pub struct PendingSequence {
    /// What the program looked like when the machine stopped.
    ///
    /// A suspended machine holds a position in code. If the script was edited
    /// between the save and the load, that position means something else — so
    /// the sequence is abandoned rather than resumed into whatever now sits
    /// there. Compared, not trusted.
    pub fingerprint: u64,
    /// Seconds still to wait.
    pub remaining: f32,
    /// The frozen machine.
    ///
    /// Opaque here on purpose: what a machine *is* belongs to the VM, and this
    /// crate defines the boundary vocabulary rather than the language. Encoding
    /// and decoding happen where the type is known.
    pub machine: Vec<u8>,
}

/// A behavior instance, as the scene records it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Encode, Decode)]
pub struct ScriptSnapshot {
    /// The behavior's own fields, by name.
    pub fields: Vec<(String, ScriptValue)>,
    /// Which state it is in, by name.
    ///
    /// `None` for a behavior that declares no states. A name the reloaded script
    /// no longer declares is dropped, and the instance starts in the first state
    /// — which is what a fresh one does.
    pub state: Option<String>,
    /// The current state's own data, by name.
    ///
    /// Only the current state's: every state shares the same slots, because a
    /// state's data exists only while the behavior is in it.
    pub state_fields: Vec<(String, ScriptValue)>,
    /// Its countdowns.
    pub timers: Vec<TimerRemaining>,
    /// A sequence stopped at an `await`, if there is one.
    pub pending: Option<PendingSequence>,
}

impl ScriptSnapshot {
    /// Whether this records nothing at all.
    ///
    /// A behavior that has never run, as distinct from one whose every field
    /// happens to be at its default — the second still has a snapshot.
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
            && self.state.is_none()
            && self.state_fields.is_empty()
            && self.timers.is_empty()
            && self.pending.is_none()
    }

    /// Reads a field by name.
    pub fn field(&self, name: &str) -> Option<&ScriptValue> {
        self.fields
            .iter()
            .find(|(known, _)| known == name)
            .map(|(_, value)| value)
    }

    /// Sets a field, replacing any value already under that name.
    pub fn with_field(mut self, name: impl Into<String>, value: ScriptValue) -> Self {
        let name = name.into();
        match self.fields.iter_mut().find(|(known, _)| *known == name) {
            Some((_, slot)) => *slot = value,
            None => self.fields.push((name, value)),
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_reads_back_by_name() {
        let snapshot = ScriptSnapshot::default().with_field("health", ScriptValue::Int(40));

        assert_eq!(snapshot.field("health"), Some(&ScriptValue::Int(40)));
        assert_eq!(snapshot.field("armour"), None);
    }

    #[test]
    fn setting_a_field_twice_replaces_it() {
        let snapshot = ScriptSnapshot::default()
            .with_field("health", ScriptValue::Int(100))
            .with_field("health", ScriptValue::Int(40));

        assert_eq!(snapshot.fields.len(), 1);
        assert_eq!(snapshot.field("health"), Some(&ScriptValue::Int(40)));
    }

    /// A behavior that has never run records nothing, which is what lets a
    /// freshly authored entity take every declared default.
    #[test]
    fn a_default_snapshot_is_empty() {
        assert!(ScriptSnapshot::default().is_empty());
        assert!(!ScriptSnapshot::default()
            .with_field("health", ScriptValue::Int(1))
            .is_empty());
    }

    /// **An `after` that fired is spent, not due now.** Loading a save must not
    /// make it fire a second time.
    #[test]
    fn a_spent_countdown_is_distinguishable_from_one_at_zero() {
        let spent = TimerRemaining {
            repeating: false,
            interval: 10.0,
            ordinal: 0,
            remaining: None,
        };
        let due = TimerRemaining {
            remaining: Some(0.0),
            ..spent.clone()
        };

        assert_ne!(spent, due);
    }
}
