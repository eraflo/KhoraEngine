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
use khora_core::script::{ErgonType, ScriptValue};

use crate::ecs::LossCause;
use crate::scene::asset_kinds::{kinds_conflict, value_holds_asset};

mod column;
mod field_error;
pub use field_error::FieldError;

pub(crate) use column::{PackedColumn, PackedColumns};

/// The kind of a run-time component's field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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

impl FieldKind {
    /// Returns the bytes the kind takes inline, `None` for a [`Value`](Self::Value),
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

    /// Returns how Ergon spells the kind — `Value`, any value, as `any`.
    pub(crate) fn spelling(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::Int => "int",
            Self::Float => "float",
            Self::Vec2 => "Vec2",
            Self::Vec3 => "Vec3",
            Self::Vec4 => "Vec4",
            Self::Quat => "Quat",
            Self::Color => "Color",
            Self::Entity => "Entity",
            Self::Value => "any",
        }
    }

    /// Returns the kind of a field of Ergon type `ty`: a scalar or an engine type is
    /// its own kind, anything else — text, an array, an optional — `Value`.
    pub fn of(ty: &khora_core::script::ErgonType) -> Self {
        use khora_core::script::ErgonType;
        match ty {
            ErgonType::Bool => Self::Bool,
            ErgonType::Int => Self::Int,
            // A time and an angle are floats in their own unit.
            ErgonType::Float | ErgonType::Duration | ErgonType::Angle => Self::Float,
            ErgonType::Entity => Self::Entity,
            ErgonType::Engine("Vec2") => Self::Vec2,
            ErgonType::Engine("Vec3") => Self::Vec3,
            ErgonType::Engine("Vec4") => Self::Vec4,
            ErgonType::Engine("Quat") => Self::Quat,
            ErgonType::Engine("Color") => Self::Color,
            _ => Self::Value,
        }
    }

    /// Returns `value` as this kind stores it, refusing a non-finite number as a Rust field does.
    ///
    /// An `int` is taken as the float it names.
    pub(crate) fn accept(self, value: &ScriptValue) -> Option<ScriptValue> {
        self.stored(value).filter(ScriptValue::is_finite)
    }

    /// Returns `value` as this kind stores it, finite or not; `None` when it is not one.
    pub(crate) fn stored(self, value: &ScriptValue) -> Option<ScriptValue> {
        match (self, value) {
            (Self::Float, ScriptValue::Int(v)) => Some(ScriptValue::Float(*v as f32)),
            (_, other) if self.holds(other) => Some(other.clone()),
            _ => None,
        }
    }

    /// Returns whether `value` is a value of this kind.
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
    /// Per field, its Ergon type where known: what tells apart values its
    /// kind does not — a `string?` and an `int?` are both any value.
    types: Vec<Option<ErgonType>>,
    /// Per field, in declaration order: where it sits in a row.
    places: Vec<Place>,
    /// Bytes of the inline part of a row.
    stride: usize,
    /// Out-of-line slots per row.
    boxed: usize,
    /// Whether a non-finite number is refused, as for authored data.
    ///
    /// Engine-written state, such as a suspended behavior's registers, is kept
    /// as computed. Set from the component's provenance.
    finite: bool,
}

impl PackedLayout {
    /// Creates a layout of `fields`.
    ///
    /// # Errors
    ///
    /// A [`LayoutError`] for a name used twice or a default not of its field's kind.
    pub fn new(mut fields: Vec<PackedField>) -> Result<Self, LayoutError> {
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
            if field.kind.accept(&field.default).is_none() {
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
        // Each default as its kind stores it — an `int` default of a `float`
        // field becomes that float — so a new row is written like any value.
        for field in &mut fields {
            if let Some(accepted) = field.kind.accept(&field.default) {
                field.default = accepted;
            }
        }
        Ok(Self {
            types: vec![None; fields.len()],
            fields,
            places,
            stride,
            boxed,
            finite: true,
        })
    }

    /// Creates a layout of `fields`, each of the Ergon type at its position in `types`.
    ///
    /// A value of another type is then refused where a row is read from a
    /// file, and a field whose type changed is defaulted where the layout changes.
    ///
    /// # Errors
    ///
    /// As [`new`](Self::new).
    pub fn typed(fields: Vec<PackedField>, types: Vec<ErgonType>) -> Result<Self, LayoutError> {
        let mut layout = Self::new(fields)?;
        for (slot, ty) in types.into_iter().enumerate().take(layout.types.len()) {
            layout.types[slot] = Some(ty);
        }
        Ok(layout)
    }

    /// Returns the Ergon type of field `slot`, where known.
    pub fn ty(&self, slot: usize) -> Option<&ErgonType> {
        self.types.get(slot).and_then(Option::as_ref)
    }

    /// Returns `value` as field `slot` stores it, or `None` when its kind refuses it.
    ///
    /// A layout keeping authored data also refuses a non-finite number.
    pub(crate) fn accept(&self, slot: usize, value: &ScriptValue) -> Option<ScriptValue> {
        let kind = self.fields.get(slot)?.kind;
        if self.finite {
            kind.accept(value)
        } else {
            kind.stored(value)
        }
    }

    /// Makes this layout keep what a component of `provenance` holds: an
    /// engine-written one keeps non-finite numbers.
    pub(crate) fn kept_for(&mut self, provenance: crate::ecs::ComponentProvenance) {
        self.finite = provenance.is_copied_on_duplicate();
    }

    /// Returns whether `other` holds the same fields, of the same types.
    pub fn same_as(&self, other: &PackedLayout) -> bool {
        self.fields == other.fields && self.types == other.types
    }

    /// Returns `value` as field `slot` keeps it, or `None` when its kind or known type refuses it.
    ///
    /// An `int` where the type holds a float, at any depth, is kept as that float.
    pub fn admit(&self, slot: usize, value: &ScriptValue) -> Option<ScriptValue> {
        self.admitted(slot, value).ok().map(|(kept, _)| kept)
    }

    /// Returns `value` as field `slot` keeps it, and whether every `int` in it
    /// kept its value exactly — or why the field refuses it.
    ///
    /// [`LossCause::Retyped`] where its kind or known type refuses the value,
    /// [`LossCause::Refused`] where it is a number that is not finite and the
    /// layout keeps authored data, [`LossCause::FieldDropped`] where there is no
    /// field `slot`. An `int` where the type holds a float, at any depth, is
    /// kept as that float ([`ErgonType::widened_and_exact`]).
    pub(crate) fn admitted(
        &self,
        slot: usize,
        value: &ScriptValue,
    ) -> Result<(ScriptValue, bool), LossCause> {
        let kind = self.fields.get(slot).ok_or(LossCause::FieldDropped)?.kind;
        let stored = kind.stored(value).ok_or(LossCause::Retyped)?;
        let (kept, exact) = match self.ty(slot) {
            Some(ty) if !ty.admits(&stored) => return Err(LossCause::Retyped),
            Some(ty) => ty.widened_and_exact(stored),
            None => (stored, true),
        };
        if self.finite && !kept.is_finite() {
            return Err(LossCause::Refused);
        }
        Ok((kept, exact))
    }

    /// Returns a `value` read from a file as field `slot` holds it, and whether it was widened.
    ///
    /// Stricter than [`admit`](Self::admit): an `int` becomes a `float` only
    /// where it converts exactly, at any depth — a value holding one a float
    /// rounds is refused whole. Anything comparing with a load reads through it.
    pub fn fit(&self, slot: usize, value: &ScriptValue) -> Option<(ScriptValue, bool)> {
        self.fit_from(slot, value, None).ok()
    }

    /// Returns `value`, of a field typed `from`, as field `slot` holds it once
    /// read, and whether it was widened — or why the field refuses it.
    ///
    /// `from` is the Ergon type the value was held under, where known. The rule
    /// of a scene load and of a relayout alike: an asset reference the two
    /// types give different kinds is [`LossCause::Retyped`]; a value holding an
    /// `int` a `float` cannot hold exactly, at the top or inside an optional,
    /// an array or a struct, is [`LossCause::Inexact`]; any other value is
    /// kept as [`admitted`](Self::admitted) keeps it. Widened is told only of
    /// a top-level `int` become a `float`.
    pub(crate) fn fit_from(
        &self,
        slot: usize,
        value: &ScriptValue,
        from: Option<&ErgonType>,
    ) -> Result<(ScriptValue, bool), LossCause> {
        let field = self.fields.get(slot).ok_or(LossCause::FieldDropped)?;
        if let (Some(from), Some(to)) = (from, self.ty(slot)) {
            if kinds_conflict(from, to) && value_holds_asset(value) {
                return Err(LossCause::Retyped);
            }
        }
        match (field.kind, value) {
            (FieldKind::Float, ScriptValue::Int(int)) => ErgonType::exact_float(*int)
                .map(|float| (ScriptValue::Float(float), true))
                .ok_or(LossCause::Inexact),
            _ => match self.admitted(slot, value)? {
                (kept, true) => Ok((kept, false)),
                (_, false) => Err(LossCause::Inexact),
            },
        }
    }

    /// Returns the fields, in declaration order.
    pub fn fields(&self) -> &[PackedField] {
        &self.fields
    }

    /// Returns the slot of the field named `name`.
    pub fn slot_of(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|field| field.name == name)
    }

    /// Returns where field `slot` sits in a row.
    pub(crate) fn place(&self, slot: usize) -> Option<Place> {
        self.places.get(slot).copied()
    }

    /// Returns the size in bytes of a row's inline part.
    pub(crate) fn stride(&self) -> usize {
        self.stride
    }

    /// Returns the number of out-of-line slots per row.
    pub(crate) fn boxed(&self) -> usize {
        self.boxed
    }
}

/// Where one field sits in a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Place {
    /// At this byte offset of the row's inline part.
    Inline(usize),
    /// At this index of the row's out-of-line slots.
    Boxed(usize),
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
