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

//! Resolved types.
//!
//! [`Ty`] is what a thing *is*; [`TypeRef`](crate::ast::TypeRef) is what the
//! author *wrote*. Keeping them apart matters because a written `Health` is
//! just a name until the checker has seen the struct, and because a `Ty` can
//! exist with no syntax behind it at all — the type of `2s + 500ms`, say.
//!
//! # Duration and Angle are types, not floats
//!
//! This is where the unit guarantee lives. `Duration` and `Angle` are distinct
//! from `Float` and from each other, so `2s + 90deg` has no valid result and
//! `float x = cooldown;` is rejected. In a language where they were all floats,
//! both would compile and misbehave at runtime — the radians/degrees mix-up is
//! a genuine class of gameplay bug, and this is what removes it.
//!
//! # `Error` stops cascades
//!
//! A malformed expression gets [`Ty::Error`], which is compatible with
//! everything. One mistake then produces one message, instead of a wave of
//! follow-on complaints about types the author never wrote.

use crate::ast::TypeRef;

/// A resolved type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    /// No value. Only valid as a return type.
    Void,
    /// 64-bit signed integer.
    Int,
    /// 32-bit float.
    Float,
    /// Boolean.
    Bool,
    /// UTF-8 text.
    Str,
    /// A span of time, in seconds. Distinct from `Float` on purpose.
    Duration,
    /// An angle, in radians. Distinct from `Float` on purpose.
    Angle,
    /// An ECS entity handle. Never null — `Entity?` is `Optional(Entity)`.
    Entity,
    /// An engine math or handle type: `Vec3`, `Quat`, `Color`, …
    Engine(&'static str),
    /// `T?`
    Optional(Box<Ty>),
    /// `T[]`
    Array(Box<Ty>),
    /// `Map<K, V>`
    Map(Box<Ty>, Box<Ty>),
    /// A user-declared struct.
    Struct(String),
    /// A user-declared behavior, the type of `this`.
    Behavior(String),
    /// Something already reported. Compatible with everything, so one mistake
    /// yields one message.
    Error,
}

/// The engine types a script may name. Listed rather than derived: the API
/// surface is a decision, and a script should not reach whatever happens to be
/// reachable.
pub const ENGINE_TYPES: &[&str] = &[
    "Vec2",
    "Vec3",
    "Vec4",
    "Quat",
    "Color",
    "Transform",
    "Mesh",
    "Material",
];

impl Ty {
    /// The name to print in a diagnostic.
    pub fn name(&self) -> String {
        match self {
            Self::Void => "void".to_owned(),
            Self::Int => "int".to_owned(),
            Self::Float => "float".to_owned(),
            Self::Bool => "bool".to_owned(),
            Self::Str => "string".to_owned(),
            Self::Duration => "Duration".to_owned(),
            Self::Angle => "Angle".to_owned(),
            Self::Entity => "Entity".to_owned(),
            Self::Engine(name) => (*name).to_owned(),
            Self::Optional(inner) => format!("{}?", inner.name()),
            Self::Array(element) => format!("{}[]", element.name()),
            Self::Map(key, value) => format!("Map<{}, {}>", key.name(), value.name()),
            Self::Struct(name) | Self::Behavior(name) => name.clone(),
            // Never surfaces: an `Error` operand means something was already
            // reported, and the check that would print this is skipped.
            Self::Error => "{unknown}".to_owned(),
        }
    }

    /// Whether a value of this type can be used where `expected` is wanted.
    ///
    /// Only one implicit widening exists — an `int` literal where a `float` is
    /// wanted. Everything else must be written out, which is what keeps
    /// `Duration` from decaying into `Float` behind the author's back.
    pub fn accepts(&self, provided: &Ty) -> bool {
        if matches!(self, Self::Error) || matches!(provided, Self::Error) {
            return true;
        }
        if self == provided {
            return true;
        }
        match (self, provided) {
            (Self::Float, Self::Int) => true,
            // `null` is typed `Optional(Error)` so it fits any optional.
            (Self::Optional(_), Self::Optional(inner)) if **inner == Self::Error => true,
            // A present value is acceptable where an optional is wanted; the
            // reverse is what the nullability check exists to reject.
            (Self::Optional(inner), other) => inner.accepts(other),
            (Self::Array(wanted), Self::Array(got)) => wanted.accepts(got),
            _ => false,
        }
    }

    /// Whether this is an optional that must be tested before use.
    pub fn is_optional(&self) -> bool {
        matches!(self, Self::Optional(_))
    }

    /// The type inside an optional, or the type itself.
    ///
    /// Used after a null test has narrowed a binding.
    pub fn unwrapped(&self) -> Ty {
        match self {
            Self::Optional(inner) => (**inner).clone(),
            other => other.clone(),
        }
    }

    /// Whether arithmetic may be performed on this type at all.
    pub fn is_numeric(&self) -> bool {
        matches!(self, Self::Int | Self::Float)
    }

    /// Whether this is one of the unit types, which follow their own
    /// arithmetic rules.
    pub fn is_unit(&self) -> bool {
        matches!(self, Self::Duration | Self::Angle)
    }
}

/// Resolves a written type against the declared structs and behaviors.
///
/// Unknown names yield `None` so the caller can report with the right span; an
/// error type is not invented here, because the caller knows whether it is
/// looking at a field, a parameter or a cast.
pub fn resolve(ty: &TypeRef, known_struct: &dyn Fn(&str) -> bool) -> Option<Ty> {
    Some(match ty {
        TypeRef::Void => Ty::Void,
        TypeRef::Named { name, .. } => named(name, known_struct)?,
        TypeRef::Optional { inner, .. } => {
            let inner = resolve(inner, known_struct)?;
            // `T??` says nothing `T?` does not, and allowing it would mean
            // every unwrap had to loop.
            if inner.is_optional() {
                return Some(inner);
            }
            Ty::Optional(Box::new(inner))
        }
        TypeRef::Array { element, .. } => Ty::Array(Box::new(resolve(element, known_struct)?)),
        TypeRef::Generic { name, args, .. } => {
            if name != "Map" || args.len() != 2 {
                return None;
            }
            Ty::Map(
                Box::new(resolve(&args[0], known_struct)?),
                Box::new(resolve(&args[1], known_struct)?),
            )
        }
    })
}

/// Resolves a bare name.
fn named(name: &str, known_struct: &dyn Fn(&str) -> bool) -> Option<Ty> {
    Some(match name {
        "int" => Ty::Int,
        "float" => Ty::Float,
        "bool" => Ty::Bool,
        "string" => Ty::Str,
        "Duration" => Ty::Duration,
        "Angle" => Ty::Angle,
        "Entity" => Ty::Entity,
        other => {
            if let Some(engine) = ENGINE_TYPES.iter().find(|t| **t == other) {
                Ty::Engine(engine)
            } else if known_struct(other) {
                Ty::Struct(other.to_owned())
            } else {
                return None;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_structs(_: &str) -> bool {
        false
    }

    #[test]
    fn an_int_widens_to_a_float_but_not_the_reverse() {
        assert!(Ty::Float.accepts(&Ty::Int));
        assert!(!Ty::Int.accepts(&Ty::Float));
    }

    /// The unit guarantee. A `Duration` is not a `Float`, so it cannot be
    /// assigned to one, and the two unit types cannot be swapped.
    #[test]
    fn units_do_not_decay_into_floats() {
        assert!(!Ty::Float.accepts(&Ty::Duration));
        assert!(!Ty::Duration.accepts(&Ty::Float));
        assert!(!Ty::Duration.accepts(&Ty::Angle));
        assert!(!Ty::Angle.accepts(&Ty::Duration));
    }

    /// A present value fits where an optional is wanted; an optional does not
    /// fit where a present value is required. That asymmetry is the whole
    /// nullability rule.
    #[test]
    fn optionals_accept_values_but_not_the_reverse() {
        let optional_entity = Ty::Optional(Box::new(Ty::Entity));
        assert!(optional_entity.accepts(&Ty::Entity));
        assert!(!Ty::Entity.accepts(&optional_entity));
    }

    /// An already-reported error is compatible with everything, so one mistake
    /// does not produce a wave of follow-on messages.
    #[test]
    fn the_error_type_absorbs_everything() {
        assert!(Ty::Int.accepts(&Ty::Error));
        assert!(Ty::Error.accepts(&Ty::Str));
    }

    #[test]
    fn resolving_maps_the_built_in_names() {
        let int = TypeRef::Named {
            name: "int".into(),
            span: crate::diagnostics::Span::empty(0),
        };
        assert_eq!(resolve(&int, &no_structs), Some(Ty::Int));
    }

    #[test]
    fn an_unknown_name_does_not_resolve() {
        let unknown = TypeRef::Named {
            name: "Wobble".into(),
            span: crate::diagnostics::Span::empty(0),
        };
        assert_eq!(resolve(&unknown, &no_structs), None);
        assert_eq!(
            resolve(&unknown, &|name| name == "Wobble"),
            Some(Ty::Struct("Wobble".into()))
        );
    }

    /// `T??` carries no more information than `T?`, and collapsing it means
    /// nothing downstream has to unwrap in a loop.
    #[test]
    fn a_doubled_optional_collapses() {
        let span = crate::diagnostics::Span::empty(0);
        let inner = TypeRef::Optional {
            inner: Box::new(TypeRef::Named {
                name: "Entity".into(),
                span,
            }),
            span,
        };
        let doubled = TypeRef::Optional {
            inner: Box::new(inner),
            span,
        };
        assert_eq!(
            resolve(&doubled, &no_structs),
            Some(Ty::Optional(Box::new(Ty::Entity)))
        );
    }

    #[test]
    fn names_read_back_the_way_they_were_written() {
        assert_eq!(Ty::Optional(Box::new(Ty::Entity)).name(), "Entity?");
        assert_eq!(Ty::Array(Box::new(Ty::Int)).name(), "int[]");
        assert_eq!(
            Ty::Map(Box::new(Ty::Str), Box::new(Ty::Int)).name(),
            "Map<string, int>"
        );
    }
}
