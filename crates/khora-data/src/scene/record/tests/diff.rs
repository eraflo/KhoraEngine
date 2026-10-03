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

//! A patch: what changed in a value, by field, and the value put back from it.

use khora_core::asset::AssetUUID;
use khora_core::ecs::PersistentId;

use super::*;

/// A struct named `name` with `fields`, in order.
fn structure(name: &str, fields: Vec<(&str, Record)>) -> Record {
    Record::Struct {
        name: name.to_owned(),
        fields: fields
            .into_iter()
            .map(|(field, value)| (field.to_owned(), value))
            .collect(),
    }
}

fn vec3(x: f32, y: f32, z: f32) -> Record {
    structure(
        "Vec3",
        vec![
            ("x", Record::F32(x)),
            ("y", Record::F32(y)),
            ("z", Record::F32(z)),
        ],
    )
}

fn transform(translation: Record, scale: Record) -> Record {
    structure(
        "Transform",
        vec![("translation", translation), ("scale", scale)],
    )
}

fn light(intensity: f32, color: &str, range: f32) -> Record {
    structure(
        "Light",
        vec![
            ("intensity", Record::F32(intensity)),
            ("color", Record::Str(color.to_owned())),
            ("range", Record::F32(range)),
        ],
    )
}

/// Whether `a` and `b` are the same record, floats compared by their bits: a
/// NaN is itself, and `0.0` is not `-0.0`.
fn same(a: &Record, b: &Record) -> bool {
    fn all<'a>(
        a: impl ExactSizeIterator<Item = &'a Record>,
        b: impl ExactSizeIterator<Item = &'a Record>,
    ) -> bool {
        a.len() == b.len() && a.zip(b).all(|(a, b)| same(a, b))
    }
    fn fields(a: &[(String, Record)], b: &[(String, Record)]) -> bool {
        a.len() == b.len()
            && a.iter()
                .zip(b)
                .all(|((na, va), (nb, vb))| na == nb && same(va, vb))
    }
    match (a, b) {
        (Record::F32(a), Record::F32(b)) => a.to_bits() == b.to_bits(),
        (Record::F64(a), Record::F64(b)) | (Record::Decimal(a), Record::Decimal(b)) => {
            a.to_bits() == b.to_bits()
        }
        (Record::Some(a), Record::Some(b)) => same(a, b),
        (Record::Seq(a), Record::Seq(b)) => all(a.iter(), b.iter()),
        (Record::Map(a), Record::Map(b)) => {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|((ka, va), (kb, vb))| same(ka, kb) && same(va, vb))
        }
        (
            Record::Struct {
                name: na,
                fields: fa,
            },
            Record::Struct {
                name: nb,
                fields: fb,
            },
        ) => na == nb && fields(fa, fb),
        (
            Record::TupleStruct {
                name: na,
                fields: fa,
            },
            Record::TupleStruct {
                name: nb,
                fields: fb,
            },
        ) => na == nb && all(fa.iter(), fb.iter()),
        (Record::Newtype { name: na, value: a }, Record::Newtype { name: nb, value: b }) => {
            na == nb && same(a, b)
        }
        (
            Record::Variant {
                enum_name: ea,
                variant: va,
                payload: pa,
            },
            Record::Variant {
                enum_name: eb,
                variant: vb,
                payload: pb,
            },
        ) => {
            ea == eb
                && va == vb
                && match (pa, pb) {
                    (VariantPayload::Unit, VariantPayload::Unit) => true,
                    (VariantPayload::Newtype(a), VariantPayload::Newtype(b)) => same(a, b),
                    (VariantPayload::Tuple(a), VariantPayload::Tuple(b)) => all(a.iter(), b.iter()),
                    (VariantPayload::Struct(a), VariantPayload::Struct(b)) => fields(a, b),
                    _ => false,
                }
        }
        (a, b) => a == b,
    }
}

/// `patch(base, diff(base, now))` is `now`, bit for bit.
fn assert_round_trips(base: &Record, now: &Record) {
    let patch_ = diff(base, now);
    let back = patch(base, &patch_);
    assert!(
        same(&back, now),
        "patching {base:?}\nwith {patch_:?}\ngave {back:?}\ninstead of {now:?}"
    );
}

/// **A save holds nothing for what did not change.** An unchanged struct is
/// an empty patch — its name, no fields.
#[test]
fn an_unchanged_struct_is_an_empty_patch() {
    let base = transform(vec3(1.0, 2.0, 3.0), vec3(1.0, 1.0, 1.0));

    assert_eq!(diff(&base, &base.clone()), structure("Transform", vec![]));
}

/// **A save holds exactly what changed.** One field changed: the patch names
/// that field, with its new value, and nothing else.
#[test]
fn one_changed_field_is_exactly_that_field() {
    let base = light(1.0, "red", 5.0);
    let now = light(2.5, "red", 5.0);

    assert_eq!(
        diff(&base, &now),
        structure("Light", vec![("intensity", Record::F32(2.5))])
    );
}

/// Several changed fields are all named, in the struct's order.
#[test]
fn every_changed_field_is_named() {
    let base = light(1.0, "red", 5.0);
    let now = light(2.5, "red", 9.0);

    assert_eq!(
        diff(&base, &now),
        structure(
            "Light",
            vec![("intensity", Record::F32(2.5)), ("range", Record::F32(9.0))]
        )
    );
}

/// A struct inside a struct of the same name is itself patched field by
/// field: moving an entity up names `translation.y`, not the whole vector.
#[test]
fn nested_same_named_structs_recurse() {
    let base = transform(vec3(1.0, 2.0, 3.0), vec3(1.0, 1.0, 1.0));
    let now = transform(vec3(1.0, 7.0, 3.0), vec3(1.0, 1.0, 1.0));

    assert_eq!(
        diff(&base, &now),
        structure(
            "Transform",
            vec![(
                "translation",
                structure("Vec3", vec![("y", Record::F32(7.0))])
            )]
        )
    );
}

/// Anything that is not two structs of one name is replaced whole: a list, a
/// map, an enum variant (even one with named fields), an option, a newtype,
/// two structs of different names.
#[test]
fn a_non_struct_is_replaced_whole() {
    let pairs = [
        (
            Record::Seq(vec![Record::I64(1), Record::I64(2), Record::I64(3)]),
            Record::Seq(vec![Record::I64(1), Record::I64(2), Record::I64(4)]),
        ),
        (
            Record::Map(vec![(Record::Str("a".into()), Record::I64(1))]),
            Record::Map(vec![(Record::Str("a".into()), Record::I64(2))]),
        ),
        (
            Record::Variant {
                enum_name: "Shape".into(),
                variant: "Box".into(),
                payload: VariantPayload::Struct(vec![
                    ("w".into(), Record::F32(1.0)),
                    ("h".into(), Record::F32(1.0)),
                ]),
            },
            Record::Variant {
                enum_name: "Shape".into(),
                variant: "Box".into(),
                payload: VariantPayload::Struct(vec![
                    ("w".into(), Record::F32(1.0)),
                    ("h".into(), Record::F32(2.0)),
                ]),
            },
        ),
        (
            Record::Some(Box::new(vec3(0.0, 0.0, 0.0))),
            Record::Some(Box::new(vec3(0.0, 1.0, 0.0))),
        ),
        (
            Record::Newtype {
                name: "Wrapper".into(),
                value: Box::new(vec3(0.0, 0.0, 0.0)),
            },
            Record::Newtype {
                name: "Wrapper".into(),
                value: Box::new(vec3(0.0, 1.0, 0.0)),
            },
        ),
        (
            vec3(1.0, 2.0, 3.0),
            structure("Vec4", vec![("x", Record::F32(1.0))]),
        ),
        (Record::I64(3), Record::Str("three".into())),
    ];
    for (base, now) in pairs {
        assert_eq!(diff(&base, &now), now, "from {base:?}");
    }
}

/// Inside a struct, a changed field that is not a struct is named with its
/// whole new value.
#[test]
fn a_changed_list_field_carries_the_whole_list() {
    let base = structure(
        "Path",
        vec![
            ("name", Record::Str("patrol".into())),
            ("points", Record::Seq(vec![Record::I64(1), Record::I64(2)])),
        ],
    );
    let now = structure(
        "Path",
        vec![
            ("name", Record::Str("patrol".into())),
            (
                "points",
                Record::Seq(vec![Record::I64(1), Record::I64(2), Record::I64(3)]),
            ),
        ],
    );

    assert_eq!(
        diff(&base, &now),
        structure(
            "Path",
            vec![(
                "points",
                Record::Seq(vec![Record::I64(1), Record::I64(2), Record::I64(3)])
            )]
        )
    );
}

/// An unchanged value that is not a struct is its own patch: replacing a
/// value by itself changes nothing.
#[test]
fn an_unchanged_non_struct_is_its_own_value() {
    let list = Record::Seq(vec![Record::Str("a".into()), Record::Unit]);
    assert_eq!(diff(&list, &list.clone()), list);
    assert_eq!(diff(&Record::I64(4), &Record::I64(4)), Record::I64(4));
}

/// A NaN that did not change did not change: a struct whose float is the very
/// same NaN is an empty patch, rather than a field rewritten on every save.
#[test]
fn an_unchanged_nan_is_no_change() {
    let base = structure(
        "Probe",
        vec![("reading", Record::F32(f32::NAN)), ("id", Record::U64(4))],
    );

    assert_eq!(diff(&base, &base.clone()), structure("Probe", vec![]));
}

/// The sign of zero is a change: a patch that dropped it would load `0.0`
/// where `-0.0` was saved.
#[test]
fn a_change_of_sign_of_zero_is_a_change() {
    let base = structure("Probe", vec![("reading", Record::F32(0.0))]);
    let now = structure("Probe", vec![("reading", Record::F32(-0.0))]);

    let patch_ = diff(&base, &now);
    assert!(
        same(
            &patch_,
            &structure("Probe", vec![("reading", Record::F32(-0.0))])
        ),
        "{patch_:?}"
    );
}

/// **The law a save rests on.** `patch(base, diff(base, now))` is `now`, over
/// every kind of value a component holds: structs nested and flat, lists,
/// maps, variants, options, entity and asset references, and floats that are
/// not finite.
#[test]
fn a_patch_of_the_diff_gives_back_now() {
    let entity = |bits: u64| Record::Entity(EntityRef::Id(PersistentId::from_bits(bits)));
    let asset = AssetUUID::new();

    let pairs: Vec<(Record, Record)> = vec![
        // Nested structs, one field deep inside.
        (
            transform(vec3(1.0, 2.0, 3.0), vec3(1.0, 1.0, 1.0)),
            transform(vec3(1.0, 2.0, 3.5), vec3(2.0, 1.0, 1.0)),
        ),
        // Unchanged.
        (light(1.0, "red", 5.0), light(1.0, "red", 5.0)),
        // Every field changed.
        (light(1.0, "red", 5.0), light(0.0, "blue", -1.0)),
        // Lists and maps inside a struct.
        (
            structure(
                "Inventory",
                vec![
                    ("items", Record::Seq(vec![Record::Str("sword".into())])),
                    (
                        "counts",
                        Record::Map(vec![(Record::Str("arrow".into()), Record::U64(12))]),
                    ),
                ],
            ),
            structure(
                "Inventory",
                vec![
                    ("items", Record::Seq(vec![])),
                    (
                        "counts",
                        Record::Map(vec![(Record::Str("arrow".into()), Record::U64(11))]),
                    ),
                ],
            ),
        ),
        // A variant switched, and one whose payload changed.
        (
            structure(
                "Collider",
                vec![(
                    "shape",
                    Record::Variant {
                        enum_name: "Shape".into(),
                        variant: "Ball".into(),
                        payload: VariantPayload::Newtype(Box::new(Record::F32(0.5))),
                    },
                )],
            ),
            structure(
                "Collider",
                vec![(
                    "shape",
                    Record::Variant {
                        enum_name: "Shape".into(),
                        variant: "Cuboid".into(),
                        payload: VariantPayload::Tuple(vec![Record::F32(1.0), Record::F32(2.0)]),
                    },
                )],
            ),
        ),
        // Options appearing and disappearing.
        (
            structure(
                "Script",
                vec![
                    ("target", Record::None),
                    ("home", Record::Some(Box::new(vec3(0.0, 0.0, 0.0)))),
                ],
            ),
            structure(
                "Script",
                vec![
                    ("target", Record::Some(Box::new(entity(7)))),
                    ("home", Record::None),
                ],
            ),
        ),
        // References: an entity re-pointed, one gone outside, an asset swapped.
        (
            structure(
                "Links",
                vec![
                    ("parent", entity(1)),
                    ("friend", entity(2)),
                    ("mesh", Record::Asset(AssetUUID::new())),
                ],
            ),
            structure(
                "Links",
                vec![
                    ("parent", entity(3)),
                    ("friend", Record::Entity(EntityRef::Outside)),
                    ("mesh", Record::Asset(asset)),
                ],
            ),
        ),
        // Non-finite floats, both ways, and a NaN kept.
        (
            structure(
                "Probe",
                vec![
                    ("a", Record::F32(f32::NAN)),
                    ("b", Record::F64(1.0)),
                    ("c", Record::F32(f32::INFINITY)),
                    ("d", Record::F32(0.0)),
                ],
            ),
            structure(
                "Probe",
                vec![
                    ("a", Record::F32(f32::NAN)),
                    ("b", Record::F64(f64::NAN)),
                    ("c", Record::F32(f32::NEG_INFINITY)),
                    ("d", Record::F32(-0.0)),
                ],
            ),
        ),
        // A field the base did not have, as an older value would lack it.
        (
            structure("Stats", vec![("health", Record::I64(100))]),
            structure(
                "Stats",
                vec![("health", Record::I64(100)), ("armour", Record::I64(5))],
            ),
        ),
        // Not a struct at the top.
        (
            Record::Seq(vec![vec3(0.0, 0.0, 0.0)]),
            Record::Seq(vec![vec3(0.0, 1.0, 0.0), vec3(2.0, 2.0, 2.0)]),
        ),
        // Structs of different names.
        (
            vec3(1.0, 1.0, 1.0),
            structure("Quat", vec![("w", Record::F32(1.0))]),
        ),
        // A struct and a non-struct.
        (light(1.0, "red", 5.0), Record::Unit),
        (Record::None, light(1.0, "red", 5.0)),
    ];

    for (base, now) in &pairs {
        assert_round_trips(base, now);
    }
}

/// **What a designer edit relies on.** A field the patch does not name takes
/// the base's value — the base as it is *now*, edited since the save was
/// taken — so an edit reaches every value the game did not change.
#[test]
fn a_field_the_patch_does_not_name_takes_the_base_value() {
    let base = light(1.0, "red", 5.0);
    let now = light(2.5, "red", 5.0);
    let patch_ = diff(&base, &now);

    let edited_base = light(1.0, "green", 12.0);

    assert_eq!(patch(&edited_base, &patch_), light(2.5, "green", 12.0));
}

/// The same, one struct down: a patch naming `translation.y` leaves the
/// edited `translation.x` and `scale` of the base in place.
#[test]
fn a_nested_field_the_patch_does_not_name_takes_the_base_value() {
    let base = transform(vec3(1.0, 2.0, 3.0), vec3(1.0, 1.0, 1.0));
    let now = transform(vec3(1.0, 7.0, 3.0), vec3(1.0, 1.0, 1.0));
    let patch_ = diff(&base, &now);

    let edited_base = transform(vec3(4.0, 2.0, 3.0), vec3(2.0, 2.0, 2.0));

    assert_eq!(
        patch(&edited_base, &patch_),
        transform(vec3(4.0, 7.0, 3.0), vec3(2.0, 2.0, 2.0))
    );
}

/// An empty patch leaves the base as it is.
#[test]
fn an_empty_patch_leaves_the_base() {
    let base = light(1.0, "red", 5.0);

    assert_eq!(patch(&base, &structure("Light", vec![])), base);
}
