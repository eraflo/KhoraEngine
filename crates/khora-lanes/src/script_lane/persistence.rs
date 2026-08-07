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

//! Between what a scene stores and what the VM runs.
//!
//! A behavior's state is part of the scene: a guard saved at forty health loads
//! at forty, not at the hundred its author typed. The [`Script`] component holds
//! that state, and it holds it **by name** while the VM addresses it by slot.
//!
//! # Why the scene keeps names and the machine keeps slots
//!
//! They answer to different pressures. Running code reads a field thousands of
//! times a second, so it reads an index. A scene file is read once and has to
//! survive the script being edited between the save and the load — the same
//! reason hot-reload matches by name, and the same conversion.
//!
//! A saved field the script no longer declares is dropped; one the script
//! declares but the save does not have takes its default. Neither is an error:
//! that is simply a scene older than the script, which is the normal state of a
//! project between two edits.
//!
//! # What a field can be
//!
//! Whatever [`bridge`] says — this module no longer decides. It once carried its
//! own translation of a value, which is how the engine ended up with four of
//! them and how two of them stopped agreeing. A list is still refused, and
//! refused **loudly**: a conversion that silently dropped one would put a guard
//! at the origin after a reload with nothing said.
//!
//! [`Script`]: khora_data::ecs::Script
//! [`bridge`]: khora_script::bridge

use khora_core::script::{PendingSequence, ScriptSnapshot, ScriptValue, TimerRemaining};
use khora_script::arena::{Persisted, PersistentStore};
use khora_script::vm::{BehaviorLayout, Program, TimerKind, Value};

use super::Pending;

/// Builds an instance's store from what a scene saved.
///
/// Fields the layout does not declare are ignored — the script dropped them
/// since the save. Fields the layout declares but the save lacks are left
/// unset, for the behavior's initialiser to fill with its declared default.
///
/// The same for a state the script no longer has and a schedule it no longer
/// declares: what is gone is gone, and what is new takes what the new script
/// says. Neither is an error — that is simply a scene older than its script,
/// which is the normal condition of a project between two edits.
pub fn store_from_snapshot(layout: &BehaviorLayout, saved: &ScriptSnapshot) -> PersistentStore {
    let mut store = PersistentStore::with_slots(layout.slot_count());

    for (name, value) in &saved.fields {
        let Some(slot) = layout.slot_of(name) else {
            log::debug!("scene holds `{name}`, which the script no longer declares");
            continue;
        };
        set(&mut store, slot, name, value);
    }

    // The state before its data, because the data is only meaningful once the
    // discriminant says which state it belongs to — a save whose state vanished
    // must not pour its data into whichever state now sits at that index.
    if let Some(name) = &saved.state {
        match layout.state_index(name) {
            Some(index) => {
                store.set(
                    layout.state_slot(),
                    Persisted::Scalar(Value::Int(index as i64)),
                );
                restore_state_fields(&mut store, layout, index, &saved.state_fields);
            }
            None => log::warn!(
                "scene has `{}` in state `{name}`, which the script no longer declares — \
                 it starts over",
                layout.name
            ),
        }
    }

    for saved in &saved.timers {
        let Some(index) = timer_index(layout, saved) else {
            log::debug!(
                "scene holds a countdown the script no longer declares, on `{}`",
                layout.name
            );
            continue;
        };
        // `None` is a spent `after`, which the running form writes as `Null` —
        // not as zero, which would be a countdown due right now, and not as
        // `Unit`, which is an unset slot the initialiser is free to arm.
        let value = match saved.remaining {
            Some(seconds) => Value::Float(seconds),
            None => Value::Null,
        };
        store.set(layout.timer_slot(index), Persisted::Scalar(value));
    }

    store
}

/// Writes the current state's own data, by the names that state declares.
fn restore_state_fields(
    store: &mut PersistentStore,
    layout: &BehaviorLayout,
    index: usize,
    saved: &[(String, ScriptValue)],
) {
    let Some(state) = layout.state_at(index) else {
        return;
    };
    for (name, value) in saved {
        let Some(offset) = state.slots.iter().position(|slot| slot == name) else {
            log::debug!("state `{}` no longer declares `{name}`", state.name);
            continue;
        };
        set(store, layout.state_data_slot() + offset, name, value);
    }
}

/// The schedule a saved countdown belongs to.
///
/// Matched on what the author wrote — `every 0.5s`, and where they wrote it —
/// then on which one among schedules written identically. Position alone would
/// swap two countdowns when a schedule is inserted above another; the interval
/// alone cannot tell `every 0.5s` inside `Patrol` from `every 0.5s` beside it,
/// and those are two different schedules.
fn timer_index(layout: &BehaviorLayout, saved: &TimerRemaining) -> Option<usize> {
    let wanted = (saved.repeating, saved.interval, saved.state.clone());
    layout
        .timers
        .iter()
        .enumerate()
        .filter(|(_, timer)| identity_of(layout, timer) == wanted)
        .nth(saved.ordinal as usize)
        .map(|(index, _)| index)
}

/// What makes one schedule the same schedule as another across a save.
///
/// One definition for both directions. The two sides used to spell it out
/// separately — the reader as a three-way comparison, the writer as a tuple —
/// and a fourth component added to one and not the other would not fail to
/// compile. It would quietly hand a saved countdown to the wrong schedule.
fn identity_of(
    layout: &BehaviorLayout,
    timer: &khora_script::vm::TimerLayout,
) -> (bool, f32, Option<String>) {
    (
        matches!(timer.kind, TimerKind::Every),
        timer.seconds,
        owner(layout, timer),
    )
}

/// The name of the state a schedule belongs to, if any.
fn owner(layout: &BehaviorLayout, timer: &khora_script::vm::TimerLayout) -> Option<String> {
    timer
        .state
        .and_then(|index| layout.state_at(index))
        .map(|state| state.name.clone())
}

/// Writes one value, saying so when it is of a kind that cannot travel.
fn set(store: &mut PersistentStore, slot: usize, name: &str, value: &ScriptValue) {
    match khora_script::bridge::to_persisted(value) {
        Ok(persisted) => store.set(slot, persisted),
        Err(why) => log::warn!("field `{name}` was not restored: {why}"),
    }
}

/// Reads an instance's store back out, for a scene to record.
///
/// Only the slots the layout names: a store may be longer than the layout after
/// a script lost a field, and writing the orphan back would resurrect it in the
/// scene file the next time it was loaded.
///
/// [`pending`](ScriptSnapshot::pending) is left empty — a suspended machine is
/// held by the instance, not by the store, and only the caller has both.
pub fn snapshot_from_store(layout: &BehaviorLayout, store: &PersistentStore) -> ScriptSnapshot {
    let state_index = match store.get(layout.state_slot()) {
        Some(Persisted::Scalar(Value::Int(index))) => usize::try_from(*index).ok(),
        _ => None,
    };
    let state = state_index.and_then(|index| layout.state_at(index));

    ScriptSnapshot {
        fields: named(layout.fields.iter().enumerate(), store, 0),
        state: state.map(|state| state.name.clone()),
        // Only the state it is *in*: every state shares these slots, so reading
        // them against another state's names would report one state's data under
        // another's labels.
        state_fields: state
            .map(|state| {
                named(
                    state.slots.iter().enumerate(),
                    store,
                    layout.state_data_slot(),
                )
            })
            .unwrap_or_default(),
        timers: countdowns(layout, store),
        pending: None,
    }
}

/// Reads a run of named slots, skipping the ones nothing has written.
fn named<'a>(
    names: impl Iterator<Item = (usize, &'a String)>,
    store: &PersistentStore,
    base: usize,
) -> Vec<(String, ScriptValue)> {
    names
        .filter_map(|(offset, name)| {
            let value = khora_script::bridge::from_persisted(store.get(base + offset)?)
                .unwrap_or_else(|why| {
                    log::warn!("field `{name}` was not recorded: {why}");
                    None
                })?;
            Some((name.clone(), value))
        })
        .collect()
}

/// Reads every countdown, keyed by what its author wrote.
fn countdowns(layout: &BehaviorLayout, store: &PersistentStore) -> Vec<TimerRemaining> {
    let mut seen: Vec<(bool, f32, Option<String>)> = Vec::new();

    layout
        .timers
        .iter()
        .enumerate()
        .filter_map(|(index, timer)| {
            let key = identity_of(layout, timer);
            let ordinal = seen.iter().filter(|held| **held == key).count() as u32;
            let (repeating, _, state) = key.clone();
            seen.push(key);

            // An unset slot is a schedule that has never been armed, which only
            // happens before the initialiser has run. Nothing to record.
            let remaining = match store.get(layout.timer_slot(index))? {
                Persisted::Scalar(Value::Float(seconds)) => Some(*seconds),
                // Spent: an `after` that fired. Recorded as such rather than
                // omitted, so loading does not re-arm it.
                Persisted::Scalar(Value::Null) => None,
                _ => return None,
            };

            Some(TimerRemaining {
                repeating,
                interval: timer.seconds,
                state,
                ordinal,
                remaining,
            })
        })
        .collect()
}

/// Writes a suspended sequence down, so a save can hold it.
///
/// Records what the program's code looked like alongside the machine, because a
/// machine is a *position* in that code — see [`resume`] for what that buys.
/// `None` when the machine cannot be encoded, which loses the sequence rather
/// than the save.
pub fn suspend(pending: &Pending, program: &Program) -> Option<PendingSequence> {
    let machine = bincode::serde::encode_to_vec(&pending.machine, bincode::config::standard())
        .map_err(|error| log::error!("a suspended sequence could not be saved: {error}"))
        .ok()?;

    Some(PendingSequence {
        fingerprint: program.fingerprint(),
        remaining: pending.remaining,
        machine,
    })
}

/// Reads a suspended sequence back, if the code it stopped in is still there.
///
/// **The fingerprint is a refusal, not a formality.** A machine holds a function
/// index, a program counter and a frame sized for that function. If the script
/// was edited between the save and the load, those name something else, and
/// resuming would run whatever now sits at that address — arbitrary code, chosen
/// by an edit nobody connected to it. So a mismatch abandons the sequence and
/// says so: the guard forgets it was attacking, which is recoverable, instead of
/// doing something no author wrote.
pub fn resume(saved: &ScriptSnapshot, program: &Program) -> Option<Pending> {
    let sequence = saved.pending.as_ref()?;

    if sequence.fingerprint != program.fingerprint() {
        log::warn!(
            "a sequence saved mid-`await` was abandoned: the script has been edited since, \
             so where it stopped no longer means the same thing"
        );
        return None;
    }

    let (machine, _) =
        bincode::serde::decode_from_slice(&sequence.machine, bincode::config::standard())
            .map_err(|error| log::error!("a suspended sequence could not be restored: {error}"))
            .ok()?;

    Some(Pending {
        machine,
        remaining: sequence.remaining,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::ecs::entity::EntityId;

    fn layout(fields: &[&str]) -> BehaviorLayout {
        BehaviorLayout {
            name: "Guard".to_owned(),
            fields: fields.iter().map(|f| (*f).to_owned()).collect(),
            states: Vec::new(),
            timers: Vec::new(),
        }
    }

    /// A scene snapshot holding just these fields.
    fn saved(pairs: &[(&str, ScriptValue)]) -> ScriptSnapshot {
        pairs
            .iter()
            .fold(ScriptSnapshot::default(), |snapshot, (name, value)| {
                snapshot.with_field(*name, value.clone())
            })
    }

    /// **The point of all of it.** A guard saved at forty health loads at
    /// forty, not at the hundred its author typed.
    #[test]
    fn a_saved_value_reaches_the_slot_that_holds_it() {
        let layout = layout(&["speed", "health"]);
        let store = store_from_snapshot(
            &layout,
            &saved(&[
                ("health", ScriptValue::Int(40)),
                ("speed", ScriptValue::Float(3.0)),
            ]),
        );

        assert_eq!(store.get(0), Some(&Persisted::Scalar(Value::Float(3.0))));
        assert_eq!(store.get(1), Some(&Persisted::Scalar(Value::Int(40))));
    }

    /// The order the scene wrote them in is irrelevant — it is a name-keyed
    /// list, and a save from before a field was reordered still loads.
    #[test]
    fn the_order_the_scene_wrote_them_in_does_not_matter() {
        let layout = layout(&["speed", "health"]);

        let forwards = store_from_snapshot(
            &layout,
            &saved(&[
                ("speed", ScriptValue::Float(3.0)),
                ("health", ScriptValue::Int(40)),
            ]),
        );
        let backwards = store_from_snapshot(
            &layout,
            &saved(&[
                ("health", ScriptValue::Int(40)),
                ("speed", ScriptValue::Float(3.0)),
            ]),
        );

        assert_eq!(forwards, backwards);
    }

    #[test]
    fn a_full_round_trip_changes_nothing() {
        let layout = layout(&["health", "name", "alive", "speed"]);
        let original = saved(&[
            ("health", ScriptValue::Int(40)),
            ("name", ScriptValue::Str("Boss".to_owned())),
            ("alive", ScriptValue::Bool(true)),
            ("speed", ScriptValue::Float(2.5)),
        ]);

        let store = store_from_snapshot(&layout, &original);
        assert_eq!(snapshot_from_store(&layout, &store).fields, original.fields);
    }

    #[test]
    fn an_entity_reference_survives_the_trip() {
        let layout = layout(&["target"]);
        let target = EntityId {
            index: 7,
            generation: 2,
        };

        let store =
            store_from_snapshot(&layout, &saved(&[("target", ScriptValue::Entity(target))]));
        assert_eq!(
            snapshot_from_store(&layout, &store).fields,
            saved(&[("target", ScriptValue::Entity(target))]).fields
        );
    }

    // ─── A scene older than the script ──────────────────────────────────────

    /// Not an error: a project between two edits is exactly this.
    #[test]
    fn a_saved_field_the_script_dropped_is_ignored() {
        let layout = layout(&["health"]);
        let store = store_from_snapshot(
            &layout,
            &saved(&[
                ("health", ScriptValue::Int(40)),
                ("stamina", ScriptValue::Int(9)),
            ]),
        );

        assert_eq!(store.len(), 1);
        assert_eq!(store.get(0), Some(&Persisted::Scalar(Value::Int(40))));
    }

    /// Left unset, so the behavior's initialiser gives it the default its
    /// author wrote — a zero written here would shadow that.
    #[test]
    fn a_field_the_save_lacks_is_left_for_the_initialiser() {
        let layout = layout(&["health", "rage"]);
        let store = store_from_snapshot(&layout, &saved(&[("health", ScriptValue::Int(40))]));

        assert_eq!(store.len(), 2, "the slot exists");
        assert_eq!(
            store.get(1),
            Some(&Persisted::Scalar(Value::Unit)),
            "and is unset rather than zero"
        );
    }

    /// A store longer than the layout is what a script that lost a field
    /// leaves behind. Writing the orphan back would resurrect it in the scene
    /// file the next time it loaded.
    #[test]
    fn an_orphan_slot_is_not_written_back() {
        let layout = layout(&["health"]);
        let mut store = PersistentStore::with_slots(2);
        store.set(0, Persisted::Scalar(Value::Int(40)));
        store.set(1, Persisted::Scalar(Value::Int(99)));

        let fields = snapshot_from_store(&layout, &store);
        assert_eq!(fields, saved(&[("health", ScriptValue::Int(40))]));
    }

    /// An unset slot yields no field at all: a scene recording "this has no
    /// value" would load it as such and shadow the initialiser's default.
    #[test]
    fn an_unset_slot_writes_no_field() {
        let layout = layout(&["health", "rage"]);
        let mut store = PersistentStore::with_slots(2);
        store.set(0, Persisted::Scalar(Value::Int(40)));

        let fields = snapshot_from_store(&layout, &store);
        assert_eq!(
            fields.fields.len(),
            1,
            "only the one that has a value: {fields:?}"
        );
        assert_eq!(fields.fields[0].0, "health");
    }

    // ─── What cannot travel ─────────────────────────────────────────────────

    /// A `Vec3` travels now that a register holds one, which is what makes a
    /// patrol target survive a save rather than resetting to the origin.
    #[test]
    fn a_vector_field_round_trips() {
        let layout = layout(&["target"]);
        let target = khora_core::math::Vec3::new(3.0, 0.0, -4.0);
        let store = store_from_snapshot(&layout, &saved(&[("target", ScriptValue::Vec3(target))]));

        assert_eq!(store.get(0), Some(&Persisted::Scalar(Value::Vec3(target))));
        assert_eq!(
            snapshot_from_store(&layout, &store).fields,
            vec![("target".to_owned(), ScriptValue::Vec3(target))]
        );
    }

    /// Refused loudly rather than dropped: a guard silently back at the origin
    /// after a reload is far harder to trace than a warning. An array is the
    /// case still waiting for a representation.
    #[test]
    fn a_value_that_cannot_be_stored_is_refused_rather_than_dropped() {
        let layout = layout(&["waypoints"]);
        let store = store_from_snapshot(
            &layout,
            &saved(&[(
                "waypoints",
                ScriptValue::Array(vec![ScriptValue::Int(1), ScriptValue::Int(2)]),
            )]),
        );

        assert_eq!(
            store.get(0),
            Some(&Persisted::Scalar(Value::Unit)),
            "unset, so the initialiser still gives it a default"
        );
    }

    #[test]
    fn an_empty_layout_produces_an_empty_store() {
        let store = store_from_snapshot(&layout(&[]), &saved(&[("gone", ScriptValue::Int(1))]));
        assert!(store.is_empty());
        assert!(snapshot_from_store(&layout(&[]), &store).is_empty());
    }
}
