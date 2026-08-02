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
//! What a register holds, plus text. A `Vec3` field is not representable yet —
//! and a conversion that silently dropped one would put a guard at the origin
//! after a reload with nothing said, so it is refused loudly instead.
//!
//! [`Script`]: khora_data::ecs::Script

use khora_core::script::ScriptValue;
use khora_script::arena::{Object, Persisted, PersistentStore};
use khora_script::vm::{BehaviorLayout, Value};

/// Builds an instance's store from what a scene saved.
///
/// Fields the layout does not declare are ignored — the script dropped them
/// since the save. Fields the layout declares but the save lacks are left
/// unset, for the behavior's initialiser to fill with its declared default.
pub fn store_from_fields(
    layout: &BehaviorLayout,
    saved: &[(String, ScriptValue)],
) -> PersistentStore {
    let mut store = PersistentStore::with_slots(layout.fields.len());

    for (name, value) in saved {
        let Some(slot) = layout.slot_of(name) else {
            log::debug!("scene holds `{name}`, which the script no longer declares");
            continue;
        };
        match to_persisted(value) {
            Some(persisted) => store.set(slot, persisted),
            None => log::warn!(
                "field `{name}` is a {} and cannot be restored yet",
                value.type_name()
            ),
        }
    }
    store
}

/// Reads an instance's store back into named fields, for a scene to save.
///
/// Only the slots the layout names: a store may be longer than the layout after
/// a script lost a field, and writing the orphan back would resurrect it in the
/// scene file the next time it was loaded.
pub fn fields_from_store(
    layout: &BehaviorLayout,
    store: &PersistentStore,
) -> Vec<(String, ScriptValue)> {
    layout
        .fields
        .iter()
        .enumerate()
        .filter_map(|(slot, name)| {
            let value = to_script_value(store.get(slot)?)?;
            Some((name.clone(), value))
        })
        .collect()
}

/// The stored form of a scene value.
fn to_persisted(value: &ScriptValue) -> Option<Persisted> {
    Some(match value {
        ScriptValue::Unit => Persisted::Scalar(Value::Unit),
        ScriptValue::Bool(flag) => Persisted::Scalar(Value::Bool(*flag)),
        ScriptValue::Int(number) => Persisted::Scalar(Value::Int(*number)),
        ScriptValue::Float(number) => Persisted::Scalar(Value::Float(*number)),
        ScriptValue::Entity(id) => Persisted::Scalar(Value::Entity(*id)),
        // By value, like every other string a field holds: an arena handle
        // would be stale by the next frame, let alone across a save.
        ScriptValue::Str(text) => Persisted::Owned(Object::Str(text.clone())),
        _ => return None,
    })
}

/// The scene form of a stored value.
///
/// An unset slot yields `None` rather than a `void` field: a scene that recorded
/// "this field has no value" would load it as such and shadow the default the
/// initialiser is meant to give it.
fn to_script_value(value: &Persisted) -> Option<ScriptValue> {
    Some(match value {
        Persisted::Scalar(Value::Unit) => return None,
        Persisted::Scalar(Value::Bool(flag)) => ScriptValue::Bool(*flag),
        Persisted::Scalar(Value::Int(number)) => ScriptValue::Int(*number),
        Persisted::Scalar(Value::Float(number)) => ScriptValue::Float(*number),
        Persisted::Scalar(Value::Entity(id)) => ScriptValue::Entity(*id),
        // A string in a register is a *reference* into the program or the
        // arena, and neither survives the frame — which is why a field's text
        // is kept owned, and why only the owned form is readable here.
        Persisted::Scalar(Value::Str(_)) | Persisted::Scalar(Value::Null) => return None,
        Persisted::Owned(Object::Str(text)) => ScriptValue::Str(text.clone()),
        Persisted::Owned(Object::Array(_)) => return None,
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

    fn saved(pairs: &[(&str, ScriptValue)]) -> Vec<(String, ScriptValue)> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect()
    }

    /// **The point of all of it.** A guard saved at forty health loads at
    /// forty, not at the hundred its author typed.
    #[test]
    fn a_saved_value_reaches_the_slot_that_holds_it() {
        let layout = layout(&["speed", "health"]);
        let store = store_from_fields(
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

        let forwards = store_from_fields(
            &layout,
            &saved(&[
                ("speed", ScriptValue::Float(3.0)),
                ("health", ScriptValue::Int(40)),
            ]),
        );
        let backwards = store_from_fields(
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

        let store = store_from_fields(&layout, &original);
        assert_eq!(fields_from_store(&layout, &store), original);
    }

    #[test]
    fn an_entity_reference_survives_the_trip() {
        let layout = layout(&["target"]);
        let target = EntityId {
            index: 7,
            generation: 2,
        };

        let store = store_from_fields(&layout, &saved(&[("target", ScriptValue::Entity(target))]));
        assert_eq!(
            fields_from_store(&layout, &store),
            saved(&[("target", ScriptValue::Entity(target))])
        );
    }

    // ─── A scene older than the script ──────────────────────────────────────

    /// Not an error: a project between two edits is exactly this.
    #[test]
    fn a_saved_field_the_script_dropped_is_ignored() {
        let layout = layout(&["health"]);
        let store = store_from_fields(
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
        let store = store_from_fields(&layout, &saved(&[("health", ScriptValue::Int(40))]));

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

        let fields = fields_from_store(&layout, &store);
        assert_eq!(fields, saved(&[("health", ScriptValue::Int(40))]));
    }

    /// An unset slot yields no field at all: a scene recording "this has no
    /// value" would load it as such and shadow the initialiser's default.
    #[test]
    fn an_unset_slot_writes_no_field() {
        let layout = layout(&["health", "rage"]);
        let mut store = PersistentStore::with_slots(2);
        store.set(0, Persisted::Scalar(Value::Int(40)));

        let fields = fields_from_store(&layout, &store);
        assert_eq!(fields.len(), 1, "only the one that has a value: {fields:?}");
        assert_eq!(fields[0].0, "health");
    }

    // ─── What cannot travel ─────────────────────────────────────────────────

    /// Refused loudly rather than dropped: a guard silently back at the origin
    /// after a reload is far harder to trace than a warning.
    #[test]
    fn a_value_that_cannot_be_stored_is_refused_rather_than_dropped() {
        let layout = layout(&["position"]);
        let store = store_from_fields(
            &layout,
            &saved(&[("position", ScriptValue::Vec3(khora_core::math::Vec3::ONE))]),
        );

        assert_eq!(
            store.get(0),
            Some(&Persisted::Scalar(Value::Unit)),
            "unset, so the initialiser still gives it a default"
        );
    }

    #[test]
    fn an_empty_layout_produces_an_empty_store() {
        let store = store_from_fields(&layout(&[]), &saved(&[("gone", ScriptValue::Int(1))]));
        assert!(store.is_empty());
        assert!(fields_from_store(&layout(&[]), &store).is_empty());
    }
}
