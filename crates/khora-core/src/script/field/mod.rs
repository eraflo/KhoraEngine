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

//! What a Rust field type is to a script.

use crate::ecs::entity::EntityId;
use crate::math::{LinearRgba, Quaternion, Vec2, Vec3, Vec4};

use super::ScriptValue;

#[cfg(test)]
mod tests;

/// The Ergon type of a component field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErgonType {
    /// `bool`
    Bool,
    /// `int`
    Int,
    /// `float`
    Float,
    /// `string`
    Str,
    /// `Entity`
    Entity,
    /// An engine type, by its Ergon name: `Vec3`, `Quat`, `Color`.
    Engine(&'static str),
    /// `T?`
    Optional(Box<ErgonType>),
    /// `T[]`
    Array(Box<ErgonType>),
}

impl std::fmt::Display for ErgonType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bool => f.write_str("bool"),
            Self::Int => f.write_str("int"),
            Self::Float => f.write_str("float"),
            Self::Str => f.write_str("string"),
            Self::Entity => f.write_str("Entity"),
            Self::Engine(name) => f.write_str(name),
            Self::Optional(inner) => write!(f, "{inner}?"),
            Self::Array(element) => write!(f, "{element}[]"),
        }
    }
}

/// A script value a field refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldValueError {
    /// What the field holds.
    pub expected: String,
    /// What was offered.
    pub found: String,
}

impl FieldValueError {
    /// A field of type `T` refused `found`.
    fn of<T: ScriptField>(found: impl Into<String>) -> Self {
        Self {
            expected: T::ergon().to_string(),
            found: found.into(),
        }
    }
}

impl std::fmt::Display for FieldValueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "expected `{}`, found `{}`", self.expected, self.found)
    }
}

/// A Rust type a script can read and write as a component field.
///
/// The one answer to "what is this field to a script": the mirror spells a
/// field with [`ergon`](Self::ergon), and a write converts through
/// [`from_script`](Self::from_script). A type that would not survive the trip
/// — `u64`, `usize`, `f64`, wider than Ergon's `int` and `float` — simply does
/// not implement it.
pub trait ScriptField: Sized {
    /// Its Ergon type.
    fn ergon() -> ErgonType;
    /// The value, as a script sees it.
    fn to_script(&self) -> ScriptValue;
    /// The field's value from a script's, refusing one it cannot hold.
    fn from_script(value: &ScriptValue) -> Result<Self, FieldValueError>;
}

impl ScriptField for f32 {
    fn ergon() -> ErgonType {
        ErgonType::Float
    }
    fn to_script(&self) -> ScriptValue {
        ScriptValue::Float(*self)
    }
    /// A non-finite value is refused: a NaN mass is not a mass, and every
    /// system reading the field would inherit it. An `int` is taken as the
    /// float it names — a `float` accepts an `int` in Ergon, and a script's
    /// `float m = 1;` holds one.
    fn from_script(value: &ScriptValue) -> Result<Self, FieldValueError> {
        match value {
            ScriptValue::Float(v) if v.is_finite() => Ok(*v),
            ScriptValue::Float(v) => Err(FieldValueError::of::<Self>(format!("{v}"))),
            ScriptValue::Int(v) => Ok(*v as f32),
            other => Err(FieldValueError::of::<Self>(other.type_name())),
        }
    }
}

macro_rules! integer_field {
    ($($ty:ty),* $(,)?) => {
        $(
            impl ScriptField for $ty {
                fn ergon() -> ErgonType {
                    ErgonType::Int
                }
                fn to_script(&self) -> ScriptValue {
                    ScriptValue::Int(i64::from(*self))
                }
                /// Range-checked: a value the field cannot hold is refused,
                /// never wrapped.
                fn from_script(value: &ScriptValue) -> Result<Self, FieldValueError> {
                    match value {
                        ScriptValue::Int(v) => <$ty>::try_from(*v).map_err(|_| FieldValueError {
                            expected: format!("an int from {} to {}", <$ty>::MIN, <$ty>::MAX),
                            found: v.to_string(),
                        }),
                        other => Err(FieldValueError::of::<Self>(other.type_name())),
                    }
                }
            }
        )*
    };
}

integer_field!(i64, i8, i16, i32, u8, u16, u32);

macro_rules! plain_field {
    ($($ty:ty => $variant:ident, $ergon:expr;)*) => {
        $(
            impl ScriptField for $ty {
                fn ergon() -> ErgonType {
                    $ergon
                }
                fn to_script(&self) -> ScriptValue {
                    ScriptValue::$variant(self.clone())
                }
                /// An engine value with a non-finite lane is refused, as a
                /// non-finite `float` is: a NaN translation is not a place.
                fn from_script(value: &ScriptValue) -> Result<Self, FieldValueError> {
                    match value {
                        ScriptValue::$variant(v) if value.is_finite() => Ok(v.clone()),
                        ScriptValue::$variant(_) => {
                            Err(FieldValueError::of::<Self>("a value with a non-finite lane"))
                        }
                        other => Err(FieldValueError::of::<Self>(other.type_name())),
                    }
                }
            }
        )*
    };
}

plain_field! {
    bool => Bool, ErgonType::Bool;
    String => Str, ErgonType::Str;
    EntityId => Entity, ErgonType::Entity;
    Vec2 => Vec2, ErgonType::Engine("Vec2");
    Vec3 => Vec3, ErgonType::Engine("Vec3");
    Vec4 => Vec4, ErgonType::Engine("Vec4");
    Quaternion => Quat, ErgonType::Engine("Quat");
    LinearRgba => Color, ErgonType::Engine("Color");
}

impl<T: ScriptField> ScriptField for Option<T> {
    fn ergon() -> ErgonType {
        ErgonType::Optional(Box::new(T::ergon()))
    }
    fn to_script(&self) -> ScriptValue {
        match self {
            Some(value) => value.to_script(),
            None => ScriptValue::Null,
        }
    }
    fn from_script(value: &ScriptValue) -> Result<Self, FieldValueError> {
        match value {
            ScriptValue::Null => Ok(None),
            other => T::from_script(other).map(Some),
        }
    }
}

impl<T: ScriptField> ScriptField for Vec<T> {
    fn ergon() -> ErgonType {
        ErgonType::Array(Box::new(T::ergon()))
    }
    fn to_script(&self) -> ScriptValue {
        ScriptValue::Array(self.iter().map(ScriptField::to_script).collect())
    }
    fn from_script(value: &ScriptValue) -> Result<Self, FieldValueError> {
        match value {
            ScriptValue::Array(items) => items.iter().map(T::from_script).collect(),
            other => Err(FieldValueError::of::<Self>(other.type_name())),
        }
    }
}

/// What `#[derive(Component)]` reaches a field's type through, so a field
/// whose type is not a [`ScriptField`] still derives — it resolves to `None`
/// instead of a compile error.
///
/// Autoref specialisation: `(&&Probe::<T>::new()).reader()` finds the impl on
/// `&Probe<T>` when `T: ScriptField`, and otherwise falls through to the one on
/// `Probe<T>`, which answers `None`. Not for direct use.
#[doc(hidden)]
pub mod probe {
    use std::marker::PhantomData;

    use super::{ErgonType, FieldValueError, ScriptField};
    use crate::script::ScriptValue;

    /// A field type, as a value to call through.
    pub struct Probe<T>(PhantomData<fn() -> T>);

    impl<T> Probe<T> {
        /// The probe of `T`.
        #[allow(clippy::new_without_default)]
        pub fn new() -> Self {
            Self(PhantomData)
        }
    }

    /// How a field reads, writes and is spelled.
    pub type Reader<T> = fn(&T) -> ScriptValue;
    /// How a field takes a script's value.
    pub type Writer<T> = fn(&ScriptValue) -> Result<T, FieldValueError>;

    /// The answer for a [`ScriptField`].
    pub trait ViaScriptField<T> {
        /// Its Ergon type.
        fn ergon_of(&self) -> Option<ErgonType>;
        /// How it reads.
        fn reader(&self) -> Option<Reader<T>>;
        /// How it is written.
        fn writer(&self) -> Option<Writer<T>>;
    }

    impl<T: ScriptField> ViaScriptField<T> for &Probe<T> {
        fn ergon_of(&self) -> Option<ErgonType> {
            Some(T::ergon())
        }
        fn reader(&self) -> Option<Reader<T>> {
            Some(T::to_script)
        }
        fn writer(&self) -> Option<Writer<T>> {
            Some(T::from_script)
        }
    }

    /// The answer for any other type: a script cannot reach it.
    pub trait ViaNothing<T> {
        /// None.
        fn ergon_of(&self) -> Option<ErgonType>;
        /// None.
        fn reader(&self) -> Option<Reader<T>>;
        /// None.
        fn writer(&self) -> Option<Writer<T>>;
    }

    impl<T> ViaNothing<T> for Probe<T> {
        fn ergon_of(&self) -> Option<ErgonType> {
            None
        }
        fn reader(&self) -> Option<Reader<T>> {
            None
        }
        fn writer(&self) -> Option<Writer<T>> {
            None
        }
    }
}
