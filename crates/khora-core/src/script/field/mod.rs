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

use crate::asset::AssetKind;
use crate::ecs::entity::EntityId;
use crate::math::{LinearRgba, Quaternion, Vec2, Vec3, Vec4};

use super::ScriptValue;

mod hints;

pub use hints::{FieldHints, FieldRange};

/// The engine types a script names, as [`ErgonType::Engine`] holds them.
const ENGINE_TYPES: [&str; 5] = ["Vec2", "Vec3", "Vec4", "Quat", "Color"];

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
    /// An asset reference of one kind: `Prefab`, `Sound`, `Material`,
    /// `Mesh`, `Texture`, `Scene`.
    Asset(AssetKind),
    /// An engine type, by its Ergon name: `Vec3`, `Quat`, `Color`.
    Engine(&'static str),
    /// `Duration` — a time a script reads in its own unit, stored as a float.
    Duration,
    /// `Angle` — an angle a script reads in its own unit, stored as a float.
    Angle,
    /// `T?`
    Optional(Box<ErgonType>),
    /// `T[]`
    Array(Box<ErgonType>),
    /// Any value — what the engine keeps of a script's own state. Spelled
    /// `<any>`, which no struct's name can be.
    Any,
    /// A struct a script declares: its name and, where known, its fields.
    ///
    /// The fields tell two versions of one struct apart. `None` where the
    /// struct holds itself, or nothing more was known.
    Struct {
        /// Its name.
        name: String,
        /// Its fields, in declaration order.
        fields: Option<Vec<(String, ErgonType)>>,
    },
}

impl ErgonType {
    /// Returns a struct known by its name only.
    pub fn named_struct(name: impl Into<String>) -> Self {
        Self::Struct {
            name: name.into(),
            fields: None,
        }
    }

    /// Parses the type spelled `text`; `None` for text that is not a type.
    ///
    /// It reads Ergon's spelling (`int`, `Vec3?`, `string[]`, `Loot`) and a
    /// [signature](Self::signature)'s, a struct with its fields
    /// (`Loot{item:string,value:int}[]`).
    pub fn parse(text: &str) -> Option<Self> {
        let mut at = 0;
        let ty = Self::parse_at(text.as_bytes(), &mut at)?;
        (at == text.len()).then_some(ty)
    }

    fn parse_at(text: &[u8], at: &mut usize) -> Option<Self> {
        if text[*at..].starts_with(b"<any>") {
            *at += b"<any>".len();
            return Some(Self::parse_suffixes(text, at, Self::Any));
        }
        let start = *at;
        while *at < text.len() && (text[*at].is_ascii_alphanumeric() || text[*at] == b'_') {
            *at += 1;
        }
        let name = std::str::from_utf8(&text[start..*at]).ok()?;
        let ty = match name {
            "" => return None,
            "bool" => Self::Bool,
            "int" => Self::Int,
            "float" => Self::Float,
            "Duration" => Self::Duration,
            "Angle" => Self::Angle,
            "string" => Self::Str,
            "Entity" => Self::Entity,
            name if AssetKind::from_ergon_name(name).is_some() => {
                Self::Asset(AssetKind::from_ergon_name(name)?)
            }
            name => match ENGINE_TYPES.iter().find(|known| **known == name) {
                Some(known) => Self::Engine(known),
                None => {
                    let mut fields = None;
                    if text.get(*at) == Some(&b'{') {
                        *at += 1;
                        let mut listed = Vec::new();
                        while text.get(*at) != Some(&b'}') {
                            if !listed.is_empty() {
                                (text.get(*at) == Some(&b',')).then_some(())?;
                                *at += 1;
                            }
                            let field_start = *at;
                            while *at < text.len()
                                && (text[*at].is_ascii_alphanumeric() || text[*at] == b'_')
                            {
                                *at += 1;
                            }
                            let field = std::str::from_utf8(&text[field_start..*at]).ok()?;
                            (!field.is_empty() && text.get(*at) == Some(&b':')).then_some(())?;
                            *at += 1;
                            listed.push((field.to_owned(), Self::parse_at(text, at)?));
                        }
                        *at += 1;
                        fields = Some(listed);
                    }
                    Self::Struct {
                        name: name.to_owned(),
                        fields,
                    }
                }
            },
        };
        Some(Self::parse_suffixes(text, at, ty))
    }

    /// Returns `ty` with the `?` and `[]` that follow it at `at`.
    fn parse_suffixes(text: &[u8], at: &mut usize, mut ty: Self) -> Self {
        loop {
            match text.get(*at) {
                Some(b'?') => {
                    *at += 1;
                    ty = Self::Optional(Box::new(ty));
                }
                Some(b'[') if text.get(*at + 1) == Some(&b']') => {
                    *at += 2;
                    ty = Self::Array(Box::new(ty));
                }
                _ => return ty,
            }
        }
    }

    /// Returns the type written whole, each struct with its fields:
    /// `Loot{item:string,value:int}[]`.
    ///
    /// A file keeps this of a field's type, so a reader tells two versions of
    /// one struct apart.
    pub fn signature(&self) -> String {
        match self {
            Self::Struct {
                name,
                fields: Some(fields),
            } => format!(
                "{name}{{{}}}",
                fields
                    .iter()
                    .map(|(field, ty)| format!("{field}:{}", ty.signature()))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Self::Optional(inner) => format!("{}?", inner.signature()),
            Self::Array(element) => format!("{}[]", element.signature()),
            other => other.to_string(),
        }
    }

    /// Returns whether `value` is a value of this type, and so fits a field of it.
    ///
    /// A `string` for an `int?`, or a struct of another shape, does not. An
    /// `int` is taken where a `float` is, as everywhere a script writes one.
    pub fn admits(&self, value: &ScriptValue) -> bool {
        match (self, value) {
            (Self::Bool, ScriptValue::Bool(_))
            | (Self::Int, ScriptValue::Int(_))
            | (
                Self::Float | Self::Duration | Self::Angle,
                ScriptValue::Float(_) | ScriptValue::Int(_),
            )
            | (Self::Str, ScriptValue::Str(_))
            | (Self::Entity, ScriptValue::Entity(_))
            // The kind is the field's, never the value's: any reference fits.
            | (Self::Asset(_), ScriptValue::Asset(_)) => true,
            (Self::Any, _) => true,
            (Self::Engine(name), value) => value.type_name() == *name,
            (Self::Optional(_), ScriptValue::Null) => true,
            (Self::Optional(inner), value) => inner.admits(value),
            (Self::Array(element), ScriptValue::Array(items)) => {
                items.iter().all(|item| element.admits(item))
            }
            (Self::Struct { fields: None, .. }, ScriptValue::Struct(_)) => true,
            (
                Self::Struct {
                    fields: Some(declared),
                    ..
                },
                ScriptValue::Struct(fields),
            ) => fields.iter().all(|(name, value)| {
                declared
                    .iter()
                    .find(|(known, _)| known == name)
                    .is_some_and(|(_, ty)| ty.admits(value))
            }),
            _ => false,
        }
    }

    /// Returns `value` as a field of this type holds it.
    ///
    /// An `int` written where a `float` is declared (a `Duration`, an `Angle`,
    /// inside an optional, an array or a struct too) is that float. Anything
    /// else is kept.
    pub fn widened(&self, value: ScriptValue) -> ScriptValue {
        self.widened_and_exact(value).0
    }

    /// Returns `value` as [`widened`](Self::widened) does, and whether every
    /// `int` it converted is its float exactly.
    ///
    /// `false` as soon as one `int`, at any depth, has no `f32` naming it
    /// ([`exact_float`](Self::exact_float)); that one is its nearest float.
    pub fn widened_and_exact(&self, value: ScriptValue) -> (ScriptValue, bool) {
        match (self, value) {
            (Self::Float | Self::Duration | Self::Angle, ScriptValue::Int(v)) => {
                match Self::exact_float(v) {
                    Some(float) => (ScriptValue::Float(float), true),
                    None => (ScriptValue::Float(v as f32), false),
                }
            }
            (Self::Optional(inner), value) if value != ScriptValue::Null => {
                inner.widened_and_exact(value)
            }
            (Self::Array(element), ScriptValue::Array(items)) => {
                let mut exact = true;
                let items = items
                    .into_iter()
                    .map(|item| {
                        let (item, item_exact) = element.widened_and_exact(item);
                        exact &= item_exact;
                        item
                    })
                    .collect();
                (ScriptValue::Array(items), exact)
            }
            (
                Self::Struct {
                    fields: Some(declared),
                    ..
                },
                ScriptValue::Struct(fields),
            ) => {
                let mut exact = true;
                let fields = fields
                    .into_iter()
                    .map(|(name, value)| {
                        let value = match declared.iter().find(|(known, _)| *known == name) {
                            Some((_, ty)) => {
                                let (value, value_exact) = ty.widened_and_exact(value);
                                exact &= value_exact;
                                value
                            }
                            None => value,
                        };
                        (name, value)
                    })
                    .collect();
                (ScriptValue::Struct(fields), exact)
            }
            (_, value) => (value, true),
        }
    }

    /// Returns the `float` naming `int` exactly, `None` where a float rounds it.
    ///
    /// Compared in `i128`, which holds every float an `i64` rounds to:
    /// `i64::MAX` rounds to 2⁶³, which no `i64` holds, so a cast back to `i64`
    /// would saturate and call it exact.
    pub fn exact_float(int: i64) -> Option<f32> {
        let float = int as f32;
        (float as i128 == i128::from(int)).then_some(float)
    }
}

// Written as its signature: a file or a program says `int[]` — a struct with
// its fields — and a reader of it needs no knowledge of this enum's shape.
impl serde::Serialize for ErgonType {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.signature())
    }
}

impl<'de> serde::Deserialize<'de> for ErgonType {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text)
            .ok_or_else(|| serde::de::Error::custom(format!("`{text}` is not an Ergon type")))
    }
}

/// As Ergon spells the type: a struct by its name.
impl std::fmt::Display for ErgonType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bool => f.write_str("bool"),
            Self::Int => f.write_str("int"),
            Self::Float => f.write_str("float"),
            Self::Str => f.write_str("string"),
            Self::Entity => f.write_str("Entity"),
            Self::Asset(kind) => f.write_str(kind.ergon_name()),
            Self::Engine(name) => f.write_str(name),
            Self::Optional(inner) => write!(f, "{inner}?"),
            Self::Array(element) => write!(f, "{element}[]"),
            Self::Any => f.write_str("<any>"),
            Self::Struct { name, .. } => f.write_str(name),
            Self::Duration => f.write_str("Duration"),
            Self::Angle => f.write_str("Angle"),
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
    /// Returns a field of type `T` refused `found`.
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
    /// Returns its Ergon type.
    fn ergon() -> ErgonType;
    /// Returns the value, as a script sees it.
    fn to_script(&self) -> ScriptValue;
    /// Returns the field's value from a script's, refusing one it cannot hold.
    ///
    /// # Errors
    ///
    /// [`FieldValueError`] when `value` is of another type, or out of the field's
    /// range.
    fn from_script(value: &ScriptValue) -> Result<Self, FieldValueError>;
}

impl ScriptField for f32 {
    fn ergon() -> ErgonType {
        ErgonType::Float
    }
    fn to_script(&self) -> ScriptValue {
        ScriptValue::Float(*self)
    }
    /// Refuses a non-finite value, which every system reading the field would
    /// inherit.
    ///
    /// An `int` is taken as the float it names, as Ergon's `float m = 1;` is.
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
                /// Converts a script value, range-checked: a value the field cannot hold is
                /// refused, never wrapped.
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
                /// Converts a script value, refusing one with a non-finite lane as a non-finite
                /// `float` is: a NaN translation is not a place.
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

/// What `#[derive(Component)]` reaches a field's type through.
///
/// A field whose type is not a [`ScriptField`] still derives: it resolves to
/// `None` instead of a compile error.
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
        /// Returns the probe of `T`.
        #[allow(clippy::new_without_default)]
        pub fn new() -> Self {
            Self(PhantomData)
        }
    }

    /// How a field reads, writes and is spelled.
    pub type FieldReader<T> = fn(&T) -> ScriptValue;
    /// How a field takes a script's value.
    pub type FieldWriter<T> = fn(&ScriptValue) -> Result<T, FieldValueError>;

    /// The answer for a [`ScriptField`].
    pub trait ViaScriptField<T> {
        /// Returns its Ergon type.
        fn ergon_of(&self) -> Option<ErgonType>;
        /// Returns how it reads.
        fn reader(&self) -> Option<FieldReader<T>>;
        /// Returns how it is written.
        fn writer(&self) -> Option<FieldWriter<T>>;
    }

    impl<T: ScriptField> ViaScriptField<T> for &Probe<T> {
        fn ergon_of(&self) -> Option<ErgonType> {
            Some(T::ergon())
        }
        fn reader(&self) -> Option<FieldReader<T>> {
            Some(T::to_script)
        }
        fn writer(&self) -> Option<FieldWriter<T>> {
            Some(T::from_script)
        }
    }

    /// The answer for any other type: a script cannot reach it.
    pub trait ViaNothing<T> {
        /// Returns `None`.
        fn ergon_of(&self) -> Option<ErgonType>;
        /// Returns `None`.
        fn reader(&self) -> Option<FieldReader<T>>;
        /// Returns `None`.
        fn writer(&self) -> Option<FieldWriter<T>>;
    }

    impl<T> ViaNothing<T> for Probe<T> {
        fn ergon_of(&self) -> Option<ErgonType> {
            None
        }
        fn reader(&self) -> Option<FieldReader<T>> {
            None
        }
        fn writer(&self) -> Option<FieldWriter<T>> {
            None
        }
    }
}

#[cfg(test)]
mod tests;
