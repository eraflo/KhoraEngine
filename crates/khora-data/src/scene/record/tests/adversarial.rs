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

//! Reads that adapt a save in ways the report must still name correctly:
//! renamed fields next to added ones, maps keyed by values that are not
//! plain names, byte strings read into fixed-size values.

use std::collections::{BTreeMap, HashMap};

use khora_core::script::{FrozenMachine, FrozenValue, PendingBody, SuspendedMachine};
use serde::{Deserialize, Serialize};

use super::*;

/// A report entry on no particular entity or component.
fn entry(path: &str, kind: ReportKind) -> ReportEntry {
    ReportEntry {
        entity: None,
        component: None,
        path: path.to_owned(),
        kind,
    }
}

fn by_path(mut entries: Vec<ReportEntry>) -> Vec<ReportEntry> {
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    entries
}

/// A struct whose `size` was once called `scale`, and which gained `color`
/// in the same release. `scale` sorts between `color` and `size`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Sprite {
    #[serde(default)]
    color: u32,
    #[serde(alias = "scale")]
    size: u32,
}

/// A save written before `color` existed and while `size` was `scale`: the
/// read renamed `size` and defaulted `color`. The report must say exactly
/// that — not that `color` was read from `scale` and `size` defaulted, which
/// tells the author the size they saved was lost.
#[test]
fn an_alias_sorting_after_an_absent_earlier_field_is_paired_with_its_owner() {
    let old = structure("Sprite", vec![("scale", Record::U64(4))]);
    let (value, entries) = resolve::<Sprite>(&old, &mut NoEntities).expect("resolves");
    assert_eq!(value, Sprite { color: 0, size: 4 });
    assert_eq!(
        by_path(entries),
        vec![
            entry("color", ReportKind::Defaulted),
            entry(
                "size",
                ReportKind::Renamed {
                    from: "scale".into()
                }
            ),
        ]
    );
}

/// A slot whose `legacy` field has since been removed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Slot {
    weight: f32,
}

/// A map keyed by entity — how the engine keys per-entity data — reports
/// what the read dropped inside its values, like a map keyed by number or by
/// name does.
#[test]
fn a_dropped_field_inside_a_map_keyed_by_entity_is_reported() {
    let mut table = IdTable::default();
    let key = entity(3, 1);
    let mut written = HashMap::new();
    written.insert(key, Slot { weight: 1.0 });
    let mut record = to_record(&written, &mut table).expect("writes");
    // The save's slot still holds the field the code has since removed.
    let Record::Map(entries) = &mut record else {
        panic!("a map writes as a map: {record:?}");
    };
    let Record::Struct { fields, .. } = &mut entries[0].1 else {
        panic!("a slot writes as a struct");
    };
    fields.push(("legacy".to_owned(), Record::U64(3)));

    let (value, entries) =
        resolve::<HashMap<EntityId, Slot>>(&record, &mut table).expect("resolves");
    assert_eq!(value[&key].weight, 1.0);
    assert_eq!(
        entries.len(),
        1,
        "the field dropped inside the entry keyed by {key:?} went unreported: {entries:?}"
    );
    assert_eq!(entries[0].kind, ReportKind::Dropped);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
enum Stance {
    #[serde(alias = "Idle")]
    Resting,
    Running,
}

/// A map keyed by an enum whose key variant was renamed: the entry's value is
/// still read, so what the read dropped inside it is still reported.
#[test]
fn a_dropped_field_inside_a_map_entry_under_a_renamed_key_is_reported() {
    let old = Record::Map(vec![(
        variant("Stance", "Idle", VariantPayload::Unit),
        structure(
            "Slot",
            vec![("weight", Record::F32(2.0)), ("legacy", Record::U64(3))],
        ),
    )]);
    let (value, entries) =
        resolve::<BTreeMap<Stance, Slot>>(&old, &mut NoEntities).expect("resolves");
    assert_eq!(value[&Stance::Resting].weight, 2.0);
    assert!(
        entries.iter().any(|e| e.kind == ReportKind::Dropped),
        "the field dropped under the renamed key went unreported: {entries:?}"
    );
}

/// Two entries whose keys become equal once read — an old name and the new
/// name of the same variant — cannot both survive: one value is lost. The
/// read either refuses the save or says something was lost; it never
/// reports a clean load that silently threw an entry away.
#[test]
fn map_entries_merged_by_a_key_rename_are_not_lost_silently() {
    let old = Record::Map(vec![
        (
            variant("Stance", "Idle", VariantPayload::Unit),
            structure("Slot", vec![("weight", Record::F32(1.0))]),
        ),
        (
            variant("Stance", "Resting", VariantPayload::Unit),
            structure("Slot", vec![("weight", Record::F32(2.0))]),
        ),
    ]);
    if let Ok((value, entries)) = resolve::<BTreeMap<Stance, Slot>>(&old, &mut NoEntities) {
        assert!(
            !entries.is_empty(),
            "two saved entries read as {} with a clean report: {value:?}",
            value.len()
        );
    }
}

/// A byte string read into a fixed-size array holds exactly as many bytes as
/// the array; more is a value the code cannot keep, and is refused like
/// surplus elements of a list are.
#[test]
fn surplus_bytes_read_into_a_fixed_size_value_are_refused() {
    let four = Record::Bytes(vec![1, 2, 3, 4]);
    assert!(
        read::<[u8; 4]>(&four).is_ok(),
        "the exact size reads from bytes"
    );
    let three = read::<[u8; 3]>(&four);
    assert!(
        three.is_err(),
        "four bytes read into [u8; 3] as {three:?}, the fourth lost"
    );
    let pair = read::<(u8, u8)>(&four);
    assert!(
        pair.is_err(),
        "four bytes read into (u8, u8) as {pair:?}, two lost"
    );
}

/// A frozen machine in the compact form of a self-describing binary format,
/// where a struct is written as the list of its fields, reads back as the
/// machine it was — as it did when the two forms were told apart by trying
/// each in turn.
#[test]
fn a_frozen_machine_round_trips_through_compact_messagepack() {
    let machine = SuspendedMachine::Frozen(FrozenMachine {
        body: PendingBody::Sequence,
        registers: vec![FrozenValue::Float(0.5)],
        frames: vec![],
        program_counter: 3,
    });
    let bytes = rmp_serde::to_vec(&machine).expect("encodes");
    let back: Result<SuspendedMachine, _> = rmp_serde::from_slice(&bytes);
    assert_eq!(back.ok(), Some(machine));
}

/// A field the code writes as `size` but reads as `extent`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Box2 {
    #[serde(rename(serialize = "size", deserialize = "extent"))]
    size: u32,
}

/// A field read from the save is not a default: whatever the report calls a
/// field whose read and write names differ, it must not tell the author the
/// value they saved was replaced by a default.
#[test]
fn a_field_read_under_its_deserialize_name_is_not_called_defaulted() {
    let saved = structure("Box2", vec![("extent", Record::U64(9))]);
    let (value, entries) = resolve::<Box2>(&saved, &mut NoEntities).expect("resolves");
    assert_eq!(value, Box2 { size: 9 });
    assert!(
        !entries.iter().any(|e| e.kind == ReportKind::Defaulted),
        "the saved 9 was read, yet the report says: {entries:?}"
    );
}

/// A pair whose `b` defaults to 7.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Pair2 {
    a: u32,
    b: u32,
}

impl Default for Pair2 {
    fn default() -> Self {
        Self { a: 0, b: 7 }
    }
}

/// A struct read from the list of its fields — a form the reader accepts —
/// that stops short of the last one: the missing field took its default, and
/// the report says so as it does for a struct read by name.
#[test]
fn a_struct_read_from_a_short_list_reports_the_defaulted_field() {
    let saved = Record::Seq(vec![Record::U64(1)]);
    let (value, entries) = resolve::<Pair2>(&saved, &mut NoEntities).expect("resolves");
    assert_eq!(value, Pair2 { a: 1, b: 7 });
    assert_eq!(entries, vec![entry("b", ReportKind::Defaulted)]);
}
