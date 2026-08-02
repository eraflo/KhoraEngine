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

//! Writing a native signature down as a constant.
//!
//! [`Ty`] cannot be one: `Optional` and `Array` box their element and `Struct`
//! owns a `String`, none of which a `const` can build. So a native's signature
//! is written in this mirror, which boxes nothing — the recursive cases point at
//! a `&'static NativeTy` instead — and is converted once when the checker asks.
//!
//! The point is not the allocation saved. It is that a declaration can be a
//! constant next to the function it describes, rather than a registration
//! executed at start-up in some other file: what a native accepts is then part
//! of its definition, and the two cannot drift.

use crate::types::Ty;

/// A type a native function can take or return, in a form a `const` can build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeTy {
    /// No value. Only valid as a return type.
    Void,
    /// `int`
    Int,
    /// `float`
    Float,
    /// `bool`
    Bool,
    /// `string`
    Str,
    /// `Duration`
    Duration,
    /// `Angle`
    Angle,
    /// `Entity`
    Entity,
    /// An engine type by name — `Vec3`, `Quat`, `Color`.
    Engine(&'static str),
    /// `T?`
    Optional(&'static NativeTy),
    /// `T[]`
    Array(&'static NativeTy),
}

impl NativeTy {
    /// The checker's type for this.
    pub fn to_ty(self) -> Ty {
        match self {
            Self::Void => Ty::Void,
            Self::Int => Ty::Int,
            Self::Float => Ty::Float,
            Self::Bool => Ty::Bool,
            Self::Str => Ty::Str,
            Self::Duration => Ty::Duration,
            Self::Angle => Ty::Angle,
            Self::Entity => Ty::Entity,
            Self::Engine(name) => Ty::Engine(name),
            Self::Optional(inner) => Ty::Optional(Box::new(inner.to_ty())),
            Self::Array(inner) => Ty::Array(Box::new(inner.to_ty())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole reason this mirror exists: a signature is a constant.
    #[test]
    fn a_signature_can_be_written_as_a_constant() {
        const PARAMS: &[NativeTy] = &[NativeTy::Float, NativeTy::Optional(&NativeTy::Entity)];

        assert_eq!(PARAMS[0].to_ty(), Ty::Float);
        assert_eq!(PARAMS[1].to_ty(), Ty::Optional(Box::new(Ty::Entity)));
    }

    #[test]
    fn every_shape_converts_to_the_checkers_type() {
        assert_eq!(NativeTy::Void.to_ty(), Ty::Void);
        assert_eq!(NativeTy::Duration.to_ty(), Ty::Duration);
        assert_eq!(NativeTy::Angle.to_ty(), Ty::Angle);
        assert_eq!(NativeTy::Engine("Vec3").to_ty(), Ty::Engine("Vec3"));
        assert_eq!(
            NativeTy::Array(&NativeTy::Int).to_ty(),
            Ty::Array(Box::new(Ty::Int))
        );
    }

    /// A `Duration` is not a `float` at the boundary either — a native declaring
    /// seconds must not silently accept a bare number.
    #[test]
    fn units_stay_distinct_across_the_boundary() {
        assert_ne!(NativeTy::Duration.to_ty(), Ty::Float);
        assert_ne!(NativeTy::Angle.to_ty(), Ty::Float);
    }
}
