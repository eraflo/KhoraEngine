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

//! Records at the edges: values a save can hold that the ordinary cases do
//! not reach.

use std::collections::BTreeMap;

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

fn resolved<T: Serialize + DeserializeOwned>(record: &Record) -> (T, Vec<ReportEntry>) {
    resolve(record, &mut NoEntities).unwrap_or_else(|e| panic!("does not resolve: {e}"))
}

/// A fixed-size value — an array, a tuple — holds exactly as many elements as
/// its type says. A save holding more has a value the code cannot keep: the
/// read either refuses it or reports what it left behind, never returns a
/// clean read that silently lost an element.
#[test]
fn surplus_elements_of_a_fixed_size_value_are_not_lost_silently() {
    let four = Record::Seq(vec![
        Record::F32(1.0),
        Record::F32(2.0),
        Record::F32(3.0),
        Record::F32(4.0),
    ]);
    let triple = Record::Seq(vec![Record::U64(1), Record::U64(2), Record::U64(3)]);
    let mut silent = Vec::new();
    if let Ok((value, entries)) = resolve::<[f32; 3]>(&four, &mut NoEntities) {
        if entries.is_empty() {
            silent.push(format!("[f32; 3] read {value:?} from four elements"));
        }
    }
    if let Ok((value, entries)) = resolve::<(u32, u32)>(&triple, &mut NoEntities) {
        if entries.is_empty() {
            silent.push(format!("(u32, u32) read {value:?} from three elements"));
        }
    }
    assert!(
        silent.is_empty(),
        "surplus elements dropped with a clean report: {silent:?}"
    );
}

/// A light as an older save wrote it: `enabled` has since been removed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Beacon {
    range: f32,
    /// Added after `enabled` was removed; unrelated to it.
    #[serde(default)]
    visible: bool,
}

/// A field the code removed and a field it added are two facts — one value
/// lost, one value invented — even when the lost value happens to equal the
/// new field's default. Calling that a rename tells the author the save's
/// value was kept, which it was not.
#[test]
fn a_dropped_field_equal_to_a_new_default_is_not_a_rename() {
    let old = structure(
        "Beacon",
        vec![
            ("range", Record::F32(5.0)),
            ("enabled", Record::Bool(false)),
        ],
    );
    let (value, entries) = resolved::<Beacon>(&old);
    assert_eq!(
        value,
        Beacon {
            range: 5.0,
            visible: false
        }
    );
    assert_eq!(
        by_path(entries),
        vec![
            entry("enabled", ReportKind::Dropped),
            entry("visible", ReportKind::Defaulted),
        ]
    );
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Settings {
    gain: f32,
}

/// A holder whose `settings` was once called `config`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Holder {
    #[serde(alias = "config")]
    settings: Settings,
}

/// A renamed field whose value also changed shape inside — here, a nested
/// field the code no longer has — was still read through its alias. The
/// report names the rename and the nested drop; it does not claim the whole
/// value was defaulted and the old one thrown away.
#[test]
fn a_renamed_field_whose_value_also_evolved_is_still_a_rename() {
    let old = structure(
        "Holder",
        vec![(
            "config",
            structure(
                "Settings",
                vec![("gain", Record::F32(0.25)), ("legacy", Record::Bool(true))],
            ),
        )],
    );
    let (value, entries) = resolved::<Holder>(&old);
    assert_eq!(value.settings.gain, 0.25, "the alias was read");
    assert_eq!(
        by_path(entries),
        vec![
            entry(
                "settings",
                ReportKind::Renamed {
                    from: "config".into()
                }
            ),
            entry("settings.legacy", ReportKind::Dropped),
        ]
    );
}

/// Two fields, each once called something else.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Pair {
    #[serde(alias = "old_a")]
    a: u32,
    #[serde(alias = "old_b")]
    b: u32,
}

/// Two renamed fields holding the same value are two renames, each from its
/// own old name — not from whichever old name the save happened to write
/// first.
#[test]
fn two_renamed_fields_with_equal_values_keep_their_own_old_names() {
    let old = structure(
        "Pair",
        vec![("old_b", Record::U64(1)), ("old_a", Record::U64(1))],
    );
    let (value, entries) = resolved::<Pair>(&old);
    assert_eq!(value, Pair { a: 1, b: 1 });
    assert_eq!(
        by_path(entries),
        vec![
            entry(
                "a",
                ReportKind::Renamed {
                    from: "old_a".into()
                }
            ),
            entry(
                "b",
                ReportKind::Renamed {
                    from: "old_b".into()
                }
            ),
        ]
    );
}

/// A renamed float field holding NaN is still the value that was read.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Sample {
    #[serde(alias = "reading")]
    value: f32,
}

#[test]
fn a_renamed_field_holding_nan_is_still_a_rename() {
    let old = structure("Sample", vec![("reading", Record::F32(f32::NAN))]);
    let (value, entries) = resolved::<Sample>(&old);
    assert!(value.value.is_nan());
    assert_eq!(
        entries,
        vec![entry(
            "value",
            ReportKind::Renamed {
                from: "reading".into()
            }
        )]
    );
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum Mode {
    #[serde(alias = "Idle")]
    Resting,
    Running,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Walker {
    mode: Mode,
}

/// A unit variant written as its bare name — the form a self-describing
/// format gives it, and one the reader accepts — is renamed through its alias
/// like any other variant, and the report says so.
#[test]
fn a_renamed_unit_variant_written_as_a_name_is_reported() {
    let old = structure("Walker", vec![("mode", text("Idle"))]);
    let (value, entries) = resolved::<Walker>(&old);
    assert_eq!(value.mode, Mode::Resting);
    assert_eq!(
        entries,
        vec![entry(
            "mode.Resting",
            ReportKind::Renamed {
                from: "Idle".into()
            }
        )]
    );
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Slot {
    weight: f32,
}

/// A field dropped inside a map's value is reported whatever the map is keyed
/// by: numbers and entities key maps as often as names do.
#[test]
fn a_dropped_field_inside_a_map_keyed_by_number_is_reported() {
    let old = Record::Map(vec![(
        Record::U64(7),
        structure(
            "Slot",
            vec![("weight", Record::F32(1.0)), ("legacy", Record::U64(3))],
        ),
    )]);
    let (value, entries) = resolved::<BTreeMap<u32, Slot>>(&old);
    assert_eq!(value[&7].weight, 1.0);
    assert!(
        !entries.is_empty(),
        "a field dropped inside map entry 7 went unreported"
    );
}

/// A frozen machine sits inside an untagged enum, so it is read through
/// serde's buffering rather than field by field. Its numbers obey the same
/// rule there: a double that an `f32` register cannot hold exactly is
/// refused, not rounded.
#[test]
fn narrowing_inside_an_untagged_enum_is_refused_too() {
    let machine = SuspendedMachine::Frozen(FrozenMachine {
        body: PendingBody::Sequence,
        registers: vec![FrozenValue::Float(0.5)],
        frames: vec![],
        program_counter: 0,
    });
    let mut record = to_record(&machine, &mut NoWriter).expect("writes");
    // Replace the register's f32 with a double it cannot hold exactly.
    fn replace_f32(record: &mut Record) -> bool {
        match record {
            Record::F32(_) => {
                *record = Record::F64(0.1);
                true
            }
            Record::Some(inner) | Record::Newtype { value: inner, .. } => replace_f32(inner),
            Record::Seq(items) | Record::TupleStruct { fields: items, .. } => {
                items.iter_mut().any(replace_f32)
            }
            Record::Struct { fields, .. } => fields.iter_mut().any(|(_, v)| replace_f32(v)),
            Record::Variant { payload, .. } => match payload {
                VariantPayload::Newtype(inner) => replace_f32(inner),
                VariantPayload::Tuple(items) => items.iter_mut().any(replace_f32),
                VariantPayload::Struct(fields) => fields.iter_mut().any(|(_, v)| replace_f32(v)),
                VariantPayload::Unit => false,
            },
            _ => false,
        }
    }
    assert!(replace_f32(&mut record), "the machine holds an f32");

    // The same number is refused where the reader sees the f32 target itself.
    assert!(read::<f32>(&Record::F64(0.1)).is_err());
    let back = read::<SuspendedMachine>(&record);
    assert!(
        back.is_err(),
        "a double 0.1 was narrowed into an f32 register: {back:?}"
    );
}

/// A writer for values that hold no entity.
struct NoWriter;

impl ReferenceWriter for NoWriter {
    fn write_entity(&mut self, entity: EntityId) -> EntityRef {
        panic!("unexpected entity {entity:?}")
    }
}
