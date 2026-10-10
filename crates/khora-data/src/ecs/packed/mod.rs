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

//! Run-time components: a component known only by its fields, declared while
//! the engine runs (by a script), stored in packed rows.

use khora_core::ecs::entity::EntityId;
use khora_core::math::{LinearRgba, Quaternion, Vec2, Vec3, Vec4};
use khora_core::script::ScriptValue;

mod column;

pub(crate) use column::{PackedColumn, PackedColumns};

/// The kind of a run-time component's field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldKind {
    /// `bool`
    Bool,
    /// `int`
    Int,
    /// `float`
    Float,
    /// `Vec2`
    Vec2,
    /// `Vec3`
    Vec3,
    /// `Vec4`
    Vec4,
    /// `Quat`
    Quat,
    /// `Color`
    Color,
    /// An entity handle.
    Entity,
    /// Any [`ScriptValue`] (string, array, struct, optional), stored out of
    /// line and deep-copied with the row.
    Value,
}

/// One field of a run-time component.
#[derive(Debug, Clone, PartialEq)]
pub struct PackedField {
    /// The field's name.
    pub name: String,
    /// What the field holds.
    pub kind: FieldKind,
    /// The value a new row starts with.
    pub default: ScriptValue,
}

/// The fields of a run-time component, and where each sits in a row.
///
/// A row is `stride` bytes holding every fixed-size field inline, little
/// endian, plus one out-of-line slot per [`FieldKind::Value`] field.
#[derive(Debug, Clone)]
pub struct PackedLayout {
    fields: Vec<PackedField>,
    /// Per field, in declaration order: where it sits in a row.
    places: Vec<Place>,
    /// Bytes of the inline part of a row.
    stride: usize,
    /// Out-of-line slots per row.
    boxed: usize,
}

/// Where one field sits in a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Place {
    /// At this byte offset of the row's inline part.
    Inline(usize),
    /// At this index of the row's out-of-line slots.
    Boxed(usize),
}

impl PackedLayout {
    /// A layout of `fields`, refusing a name used twice or a default that is not
    /// of its field's kind.
    pub fn new(fields: Vec<PackedField>) -> Result<Self, LayoutError> {
        let mut places = Vec::with_capacity(fields.len());
        let (mut stride, mut boxed) = (0, 0);
        for (at, field) in fields.iter().enumerate() {
            if fields[..at]
                .iter()
                .any(|earlier| earlier.name == field.name)
            {
                return Err(LayoutError::DuplicateField {
                    field: field.name.clone(),
                });
            }
            if !field.kind.holds(&field.default) {
                return Err(LayoutError::DefaultOfAnotherKind {
                    field: field.name.clone(),
                    kind: field.kind,
                    found: field.default.type_name().to_owned(),
                });
            }
            places.push(match field.kind.size() {
                Some(size) => {
                    stride += size;
                    Place::Inline(stride - size)
                }
                None => {
                    boxed += 1;
                    Place::Boxed(boxed - 1)
                }
            });
        }
        Ok(Self {
            fields,
            places,
            stride,
            boxed,
        })
    }

    /// The fields, in declaration order.
    pub fn fields(&self) -> &[PackedField] {
        &self.fields
    }

    /// The slot of the field named `name`.
    pub fn slot_of(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|field| field.name == name)
    }

    /// Where field `slot` sits in a row.
    pub(crate) fn place(&self, slot: usize) -> Option<Place> {
        self.places.get(slot).copied()
    }

    /// Bytes of a row's inline part.
    pub(crate) fn stride(&self) -> usize {
        self.stride
    }

    /// Out-of-line slots per row.
    pub(crate) fn boxed(&self) -> usize {
        self.boxed
    }
}

impl FieldKind {
    /// Bytes the kind takes inline, `None` for a [`Value`](Self::Value),
    /// stored out of line.
    fn size(self) -> Option<usize> {
        Some(match self {
            Self::Bool => 1,
            Self::Int | Self::Vec2 | Self::Entity => 8,
            Self::Float => 4,
            Self::Vec3 => 12,
            Self::Vec4 | Self::Quat | Self::Color => 16,
            Self::Value => return None,
        })
    }

    /// Whether `value` is a value of this kind.
    pub(crate) fn holds(self, value: &ScriptValue) -> bool {
        matches!(
            (self, value),
            (Self::Bool, ScriptValue::Bool(_))
                | (Self::Int, ScriptValue::Int(_))
                | (Self::Float, ScriptValue::Float(_))
                | (Self::Vec2, ScriptValue::Vec2(_))
                | (Self::Vec3, ScriptValue::Vec3(_))
                | (Self::Vec4, ScriptValue::Vec4(_))
                | (Self::Quat, ScriptValue::Quat(_))
                | (Self::Color, ScriptValue::Color(_))
                | (Self::Entity, ScriptValue::Entity(_))
                | (Self::Value, _)
        )
    }
}

/// Writes `value`, of an inline kind, into `bytes` — exactly its size.
pub(crate) fn encode(value: &ScriptValue, bytes: &mut [u8]) {
    let mut at = 0;
    let mut put = |chunk: &[u8]| {
        bytes[at..at + chunk.len()].copy_from_slice(chunk);
        at += chunk.len();
    };
    match value {
        ScriptValue::Bool(b) => put(&[u8::from(*b)]),
        ScriptValue::Int(i) => put(&i.to_le_bytes()),
        ScriptValue::Float(f) => put(&f.to_le_bytes()),
        ScriptValue::Vec2(v) => [v.x, v.y].iter().for_each(|f| put(&f.to_le_bytes())),
        ScriptValue::Vec3(v) => [v.x, v.y, v.z].iter().for_each(|f| put(&f.to_le_bytes())),
        ScriptValue::Vec4(v) => [v.x, v.y, v.z, v.w]
            .iter()
            .for_each(|f| put(&f.to_le_bytes())),
        ScriptValue::Quat(q) => [q.x, q.y, q.z, q.w]
            .iter()
            .for_each(|f| put(&f.to_le_bytes())),
        ScriptValue::Color(c) => [c.r, c.g, c.b, c.a]
            .iter()
            .for_each(|f| put(&f.to_le_bytes())),
        ScriptValue::Entity(e) => {
            put(&e.index.to_le_bytes());
            put(&e.generation.to_le_bytes());
        }
        // Out-of-line kinds never reach the inline bytes.
        _ => {}
    }
}

/// Reads a value of the inline `kind` from `bytes`.
pub(crate) fn decode(kind: FieldKind, bytes: &[u8]) -> ScriptValue {
    let word = |at: usize| -> [u8; 4] {
        let mut word = [0; 4];
        word.copy_from_slice(&bytes[at..at + 4]);
        word
    };
    let f = |at: usize| f32::from_le_bytes(word(at * 4));
    match kind {
        FieldKind::Bool => ScriptValue::Bool(bytes[0] != 0),
        FieldKind::Int => {
            let mut long = [0; 8];
            long.copy_from_slice(&bytes[..8]);
            ScriptValue::Int(i64::from_le_bytes(long))
        }
        FieldKind::Float => ScriptValue::Float(f(0)),
        FieldKind::Vec2 => ScriptValue::Vec2(Vec2::new(f(0), f(1))),
        FieldKind::Vec3 => ScriptValue::Vec3(Vec3::new(f(0), f(1), f(2))),
        FieldKind::Vec4 => ScriptValue::Vec4(Vec4::new(f(0), f(1), f(2), f(3))),
        FieldKind::Quat => ScriptValue::Quat(Quaternion {
            x: f(0),
            y: f(1),
            z: f(2),
            w: f(3),
        }),
        FieldKind::Color => ScriptValue::Color(LinearRgba::new(f(0), f(1), f(2), f(3))),
        FieldKind::Entity => ScriptValue::Entity(EntityId {
            index: u32::from_le_bytes(word(0)),
            generation: u32::from_le_bytes(word(4)),
        }),
        FieldKind::Value => ScriptValue::Unit,
    }
}

/// Why a [`PackedLayout`] was refused.
#[derive(Debug, Clone, PartialEq)]
pub enum LayoutError {
    /// Two fields share a name.
    DuplicateField {
        /// The name used twice.
        field: String,
    },
    /// A field's default is not a value of the field's kind.
    DefaultOfAnotherKind {
        /// The field.
        field: String,
        /// Its kind.
        kind: FieldKind,
        /// The type of the default it was given (`ScriptValue::type_name`).
        found: String,
    },
}

/// A value refused by a run-time component's field: not of the field's kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError {
    /// The field written.
    pub field: String,
    /// The kind it holds.
    pub expected: FieldKind,
    /// The type of the value offered (`ScriptValue::type_name`).
    pub found: String,
}
