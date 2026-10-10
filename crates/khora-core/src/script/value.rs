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

//! The value vocabulary crossing the script/engine boundary.
//!
//! Deliberately *not* a scripting language's internal value type. A VM's value
//! is shaped by how the VM runs — registers, arena handles, tagged unions — and
//! none of that means anything to the engine applying the effect. This type is
//! what the engine understands: engine math types, an [`EntityId`], and the
//! scalars that survive the trip.
//!
//! Keeping the two apart is why [`WorldCommand`] is not Ergon-specific. A second
//! language, or the editor replaying a recorded session, emits the same commands
//! without inheriting Ergon's runtime.
//!
//! [`WorldCommand`]: super::WorldCommand
//! [`EntityId`]: crate::ecs::entity::EntityId

use serde::{Deserialize, Serialize};

use crate::ecs::entity::EntityId;
use crate::math::{LinearRgba, Quaternion, Vec2, Vec3, Vec4};

/// A value handed to the engine by a script.
///
/// Serializable, because this is also the form a behavior's authored fields
/// take in a scene file — a designer setting `speed = 3.0` in the inspector and
/// a script writing to a component are the same value crossing the same
/// boundary, and giving them two representations would mean two things to keep
/// in step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ScriptValue {
    /// No value.
    Unit,
    /// `bool`
    Bool(bool),
    /// `int`
    Int(i64),
    /// `float`
    Float(f32),
    /// `string`
    ///
    /// Owned: the script's copy lives in the frame arena and is gone by the
    /// time the command is applied.
    Str(String),
    /// `Vec2`
    Vec2(Vec2),
    /// `Vec3`
    Vec3(Vec3),
    /// `Vec4`
    Vec4(Vec4),
    /// `Quat`
    Quat(Quaternion),
    /// `Color`
    Color(LinearRgba),
    /// An entity handle.
    Entity(EntityId),
    /// A list of values.
    Array(Vec<ScriptValue>),
    /// Named fields — a `struct`, or the fields of a component being written.
    ///
    /// A list rather than a map because it is built once and read once, and
    /// because declaration order is worth keeping: it is the order the author
    /// wrote, which is what a diagnostic should echo back.
    ///
    /// Carrying *some* of a component's fields is the normal case, not a
    /// degenerate one. `health.current = 50` is a write to one field, and the
    /// applier merges it onto what the component already holds rather than
    /// demanding the script restate every field it did not touch.
    Struct(Vec<(String, ScriptValue)>),
    /// The absent optional.
    Null,
}

impl ScriptValue {
    /// Whether every number the value carries is finite — a `float`, every
    /// lane of an engine value, every element of an array or a struct. Text,
    /// handles and `null` carry none.
    pub fn is_finite(&self) -> bool {
        match self {
            Self::Float(v) => v.is_finite(),
            Self::Vec2(v) => v.x.is_finite() && v.y.is_finite(),
            Self::Vec3(v) => v.x.is_finite() && v.y.is_finite() && v.z.is_finite(),
            Self::Vec4(v) => [v.x, v.y, v.z, v.w].iter().all(|lane| lane.is_finite()),
            Self::Quat(q) => [q.x, q.y, q.z, q.w].iter().all(|lane| lane.is_finite()),
            Self::Color(c) => [c.r, c.g, c.b, c.a].iter().all(|lane| lane.is_finite()),
            Self::Array(items) => items.iter().all(Self::is_finite),
            Self::Struct(fields) => fields.iter().all(|(_, value)| value.is_finite()),
            _ => true,
        }
    }

    /// The type name, as an author would write it.
    ///
    /// Used by the applier when a component patch arrives with the wrong shape:
    /// the message names both types rather than reporting a bare mismatch.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Unit => "void",
            Self::Bool(_) => "bool",
            Self::Int(_) => "int",
            Self::Float(_) => "float",
            Self::Str(_) => "string",
            Self::Vec2(_) => "Vec2",
            Self::Vec3(_) => "Vec3",
            Self::Vec4(_) => "Vec4",
            Self::Quat(_) => "Quat",
            Self::Color(_) => "Color",
            Self::Entity(_) => "Entity",
            Self::Array(_) => "array",
            Self::Struct(_) => "struct",
            Self::Null => "null",
        }
    }
}

macro_rules! accessor {
    ($name:ident, $variant:ident, $ty:ty, $doc:literal) => {
        #[doc = $doc]
        pub fn $name(&self) -> Option<$ty> {
            match self {
                Self::$variant(value) => Some(*value),
                _ => None,
            }
        }
    };
}

impl ScriptValue {
    accessor!(as_bool, Bool, bool, "The value as a `bool`, if it is one.");
    accessor!(as_int, Int, i64, "The value as an `int`, if it is one.");
    accessor!(
        as_float,
        Float,
        f32,
        "The value as a `float`, if it is one."
    );
    accessor!(as_vec2, Vec2, Vec2, "The value as a `Vec2`, if it is one.");
    accessor!(as_vec3, Vec3, Vec3, "The value as a `Vec3`, if it is one.");
    accessor!(as_vec4, Vec4, Vec4, "The value as a `Vec4`, if it is one.");
    accessor!(
        as_quat,
        Quat,
        Quaternion,
        "The value as a `Quat`, if it is one."
    );
    accessor!(
        as_color,
        Color,
        LinearRgba,
        "The value as a `Color`, if it is one."
    );
    accessor!(
        as_entity,
        Entity,
        EntityId,
        "The value as an `Entity`, if it is one."
    );

    /// The value as a string slice, if it is one.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(text) => Some(text),
            _ => None,
        }
    }

    /// The elements, if the value is an array.
    pub fn as_array(&self) -> Option<&[ScriptValue]> {
        match self {
            Self::Array(values) => Some(values),
            _ => None,
        }
    }

    /// The named fields, if the value is a struct.
    pub fn as_fields(&self) -> Option<&[(String, ScriptValue)]> {
        match self {
            Self::Struct(fields) => Some(fields),
            _ => None,
        }
    }
}
