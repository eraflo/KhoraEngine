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

//! A boundary value into a slot of a declared type.
//!
//! [`ScriptValue::Struct`] names its fields but not its struct — it is the same
//! shape a component patch uses — so only the declared type of the slot it
//! goes into can say which struct a saved value is. That type also says what
//! a field the saved value lacks starts at: its default, else its zero. A
//! saved field the struct no longer declares is dropped, with a warning naming
//! it.

use khora_core::script::ScriptValue;

use super::{to_owned, Unrepresentable};
use crate::arena::{Owned, Persisted};
use crate::vm::StructLayout;

/// The stored form of `value`, for a slot declared `ty`.
pub fn to_persisted_as(
    value: &ScriptValue,
    ty: &str,
    structs: &[StructLayout],
) -> Result<Persisted, Unrepresentable> {
    Ok(match to_owned_as(value, ty, structs)? {
        Owned::Scalar(scalar) => Persisted::Scalar(scalar),
        owned => Persisted::Owned(owned),
    })
}

/// The owned form of `value`, for a slot declared `ty` (as written) — refused
/// when it does not fit that type all the way down: a struct's field retyped
/// since the save does not load as the old kind.
pub fn to_owned_as(
    value: &ScriptValue,
    ty: &str,
    structs: &[StructLayout],
) -> Result<Owned, Unrepresentable> {
    let owned = converted(value, ty, structs)?;
    if !owned.fits(ty, structs) {
        return Err(Unrepresentable::Kind {
            kind: "value of another type",
            into: "this field",
        });
    }
    Ok(owned)
}

fn converted(
    value: &ScriptValue,
    ty: &str,
    structs: &[StructLayout],
) -> Result<Owned, Unrepresentable> {
    if let Some(inner) = ty.strip_suffix('?') {
        if matches!(value, ScriptValue::Null) {
            return to_owned(value);
        }
        return converted(value, inner, structs);
    }
    if let Some(element) = ty.strip_suffix("[]") {
        if let ScriptValue::Array(items) = value {
            return Ok(Owned::Array(
                items
                    .iter()
                    .map(|item| converted(item, element, structs))
                    .collect::<Result<_, _>>()?,
            ));
        }
    }
    let (Some(layout), ScriptValue::Struct(saved)) =
        (structs.iter().find(|layout| layout.name == ty), value)
    else {
        return to_owned(value);
    };

    for (field, _) in saved {
        if !layout.fields.iter().any(|(declared, _)| declared == field) {
            log::warn!(
                "`{}` no longer declares `{field}`: its saved value is dropped",
                layout.name
            );
        }
    }
    let fields = layout
        .fields
        .iter()
        .enumerate()
        .map(|(slot, (field, field_ty))| {
            let value = match saved.iter().find(|(name, _)| name == field) {
                Some((_, value)) => converted(value, field_ty, structs)?,
                None => match layout.defaults.get(slot) {
                    Some(Some(default)) => default.clone(),
                    _ => Owned::zero(field_ty).ok_or(Unrepresentable::Kind {
                        kind: "struct missing a field with no default",
                        into: "this field",
                    })?,
                },
            };
            Ok((field.clone(), value))
        })
        .collect::<Result<_, Unrepresentable>>()?;
    Ok(Owned::Struct {
        name: layout.name.clone(),
        fields,
    })
}
