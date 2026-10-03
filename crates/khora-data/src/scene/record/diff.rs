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

//! What changed in a value, field by field, and putting it back.
//!
//! Two structs of the same name differ field by field: a patch lists only the
//! fields that changed, recursively through same-named structs. Any other value
//! is replaced whole. `patch(base, &diff(base, now)) == now` for every pair.
//!
//! The base is always a value as this build reads it — named, typed — while
//! a patch may come back from a format that drops struct names, a struct
//! written as a map keyed by field name. Against a base struct such a map is
//! that struct's patch: the base says what the value is. Two maps are never
//! merged — a map may be a real map, whose removed keys a patch cannot say.

use super::value::{Record, VariantPayload};

/// The patch that turns `base` into `now`.
///
/// An unchanged struct is an empty patch; an unchanged value of any other kind
/// is its own value — patching with it changes nothing.
pub fn diff(base: &Record, now: &Record) -> Record {
    match (base, now) {
        (
            Record::Struct {
                name: base_name,
                fields: base_fields,
            },
            Record::Struct { name, fields },
        ) if base_name == name => Record::Struct {
            name: name.clone(),
            fields: fields
                .iter()
                .filter_map(|(field, value)| match field_named(base_fields, field) {
                    Some(old) if same(old, value) => None,
                    Some(old) => Some((field.clone(), diff(old, value))),
                    None => Some((field.clone(), value.clone())),
                })
                .collect(),
        },
        _ => now.clone(),
    }
}

/// `base`, with `patch` applied: every field the patch names takes the
/// patch's value, every other keeps the base's.
pub fn patch(base: &Record, patch: &Record) -> Record {
    let Record::Struct {
        name,
        fields: base_fields,
    } = base
    else {
        return patch.clone();
    };
    let Some(fields) = patch_fields(name, patch) else {
        return patch.clone();
    };
    let mut patched: Vec<(String, Record)> = base_fields
        .iter()
        .map(|(field, old)| match field_in(&fields, field) {
            Some(new) => (field.clone(), self::patch(old, new)),
            None => (field.clone(), old.clone()),
        })
        .collect();
    // A field the base does not have — the type gained it after the base was
    // written — is the patch's own.
    patched.extend(
        fields
            .iter()
            .filter(|(field, _)| field_named(base_fields, field).is_none())
            .map(|(field, value)| ((*field).to_owned(), (*value).clone())),
    );
    Record::Struct {
        name: name.clone(),
        fields: patched,
    }
}

/// The fields `patch` sets on a struct named `name`: those of a struct of that
/// name, or of a map keyed by field name — the struct, written by a format
/// that drops names.
fn patch_fields<'r>(name: &str, patch: &'r Record) -> Option<Vec<(&'r str, &'r Record)>> {
    match patch {
        Record::Struct {
            name: patch_name,
            fields,
        } if patch_name == name => Some(
            fields
                .iter()
                .map(|(field, value)| (field.as_str(), value))
                .collect(),
        ),
        Record::Map(entries) => entries
            .iter()
            .map(|(key, value)| match key {
                Record::Str(field) => Some((field.as_str(), value)),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

fn field_in<'r>(fields: &[(&str, &'r Record)], name: &str) -> Option<&'r Record> {
    fields
        .iter()
        .find(|(field, _)| *field == name)
        .map(|(_, value)| *value)
}

fn field_named<'r>(fields: &'r [(String, Record)], name: &str) -> Option<&'r Record> {
    fields
        .iter()
        .find(|(field, _)| field == name)
        .map(|(_, value)| value)
}

/// Whether two values are the same value — floats by their bits, so an
/// unchanged `NaN` is no change and `0.0` becoming `-0.0` is one.
pub(crate) fn same(a: &Record, b: &Record) -> bool {
    fn all(a: &[Record], b: &[Record]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same(a, b))
    }
    fn named(a: &[(String, Record)], b: &[(String, Record)]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|((x, a), (y, b))| x == y && same(a, b))
    }
    match (a, b) {
        (Record::F32(a), Record::F32(b)) => a.to_bits() == b.to_bits(),
        (Record::F64(a), Record::F64(b)) | (Record::Decimal(a), Record::Decimal(b)) => {
            a.to_bits() == b.to_bits()
        }
        (Record::Some(a), Record::Some(b)) => same(a, b),
        (Record::Seq(a), Record::Seq(b)) => all(a, b),
        (Record::Map(a), Record::Map(b)) => {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|((ka, va), (kb, vb))| same(ka, kb) && same(va, vb))
        }
        (Record::Struct { name: x, fields: a }, Record::Struct { name: y, fields: b }) => {
            x == y && named(a, b)
        }
        (
            Record::TupleStruct { name: x, fields: a },
            Record::TupleStruct { name: y, fields: b },
        ) => x == y && all(a, b),
        (Record::Newtype { name: x, value: a }, Record::Newtype { name: y, value: b }) => {
            x == y && same(a, b)
        }
        (
            Record::Variant {
                enum_name: e,
                variant: v,
                payload: a,
            },
            Record::Variant {
                enum_name: f,
                variant: w,
                payload: b,
            },
        ) => {
            e == f
                && v == w
                && match (a, b) {
                    (VariantPayload::Unit, VariantPayload::Unit) => true,
                    (VariantPayload::Newtype(a), VariantPayload::Newtype(b)) => same(a, b),
                    (VariantPayload::Tuple(a), VariantPayload::Tuple(b)) => all(a, b),
                    (VariantPayload::Struct(a), VariantPayload::Struct(b)) => named(a, b),
                    _ => false,
                }
        }
        _ => a == b,
    }
}
