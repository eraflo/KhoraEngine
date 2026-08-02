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

//! What a Rust type means to a script.
//!
//! The bridge `#[ergon_fn]` compiles against. A macro that read the *written*
//! type name — matching the token `f32` and hoping — would break on a type
//! alias, on `super::Vec3`, and on anything generic; and it could only ever
//! know the types the macro author listed. Asking the type itself instead
//! means the mapping is resolved by the compiler, and a crate can expose its
//! own type to scripts by implementing this trait, without the macro learning
//! anything new.
//!
//! [`TY`](ScriptType::TY) is an associated **constant**, which is what lets a
//! generated signature be a `static` rather than something built at start-up.

use khora_core::ecs::entity::EntityId;

use super::{NativeContext, NativeError, NativeTy};
use crate::vm::Value;

/// A Rust type a script can pass or receive.
pub trait ScriptType: Sized {
    /// How the type is written in Ergon.
    const TY: NativeTy;

    /// Reads a value the VM produced.
    ///
    /// The checker has already proved the call well-typed, so a mismatch here
    /// means the registry and the program disagree — an engine fault, not the
    /// author's mistake, and the message says so.
    fn from_value(value: Value) -> Result<Self, NativeError>;

    /// Turns a result into a value the VM can hold.
    ///
    /// Takes the context because a type too large for a register has to be put
    /// somewhere, and that somewhere is the frame arena.
    fn to_value(self, context: &mut NativeContext<'_>) -> Result<Value, NativeError>;
}

/// The message for a value that arrived in the wrong shape.
fn mismatch(expected: &str, found: Value) -> NativeError {
    NativeError::new(format!(
        "an engine function expected {expected} but the call supplied {} — \
         the program and the engine disagree about its signature",
        found.type_name()
    ))
}

impl ScriptType for f32 {
    const TY: NativeTy = NativeTy::Float;

    fn from_value(value: Value) -> Result<Self, NativeError> {
        value.as_float().ok_or_else(|| mismatch("a float", value))
    }

    fn to_value(self, _: &mut NativeContext<'_>) -> Result<Value, NativeError> {
        Ok(Value::Float(self))
    }
}

impl ScriptType for i64 {
    const TY: NativeTy = NativeTy::Int;

    fn from_value(value: Value) -> Result<Self, NativeError> {
        // Not `as_float().round()`: the language has no implicit narrowing, and
        // a native must not invent one where the checker refused it.
        value.as_int().ok_or_else(|| mismatch("an int", value))
    }

    fn to_value(self, _: &mut NativeContext<'_>) -> Result<Value, NativeError> {
        Ok(Value::Int(self))
    }
}

impl ScriptType for bool {
    const TY: NativeTy = NativeTy::Bool;

    fn from_value(value: Value) -> Result<Self, NativeError> {
        value.as_bool().ok_or_else(|| mismatch("a bool", value))
    }

    fn to_value(self, _: &mut NativeContext<'_>) -> Result<Value, NativeError> {
        Ok(Value::Bool(self))
    }
}

impl ScriptType for EntityId {
    const TY: NativeTy = NativeTy::Entity;

    fn from_value(value: Value) -> Result<Self, NativeError> {
        value
            .as_entity()
            .ok_or_else(|| mismatch("an Entity", value))
    }

    fn to_value(self, _: &mut NativeContext<'_>) -> Result<Value, NativeError> {
        Ok(Value::Entity(self))
    }
}

impl ScriptType for () {
    const TY: NativeTy = NativeTy::Void;

    fn from_value(_: Value) -> Result<Self, NativeError> {
        Ok(())
    }

    fn to_value(self, _: &mut NativeContext<'_>) -> Result<Value, NativeError> {
        Ok(Value::Unit)
    }
}

/// `T?` — an absent value is `null` rather than a missing argument.
impl<T: ScriptType> ScriptType for Option<T> {
    const TY: NativeTy = NativeTy::Optional(&T::TY);

    fn from_value(value: Value) -> Result<Self, NativeError> {
        if value.is_null() {
            return Ok(None);
        }
        T::from_value(value).map(Some)
    }

    fn to_value(self, context: &mut NativeContext<'_>) -> Result<Value, NativeError> {
        match self {
            Some(inner) => inner.to_value(context),
            None => Ok(Value::Null),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip<T: ScriptType + PartialEq + std::fmt::Debug + Copy>(value: T) {
        let mut commands = khora_core::script::CommandBuffer::new();
        let mut arena = crate::arena::Arena::new();
        let mut context = NativeContext {
            commands: &mut commands,
            arena: &mut arena,
            entity: None,
        };

        let encoded = value.to_value(&mut context).expect("encodes");
        assert_eq!(T::from_value(encoded).expect("decodes"), value);
    }

    #[test]
    fn the_scalar_types_round_trip() {
        round_trip(1.5f32);
        round_trip(42i64);
        round_trip(true);
        round_trip(EntityId {
            index: 7,
            generation: 2,
        });
    }

    /// The whole point of the associated constant: a signature can be written
    /// as one, so a generated declaration is a `static`.
    #[test]
    fn a_signature_can_be_built_at_compile_time() {
        const PARAMS: &[NativeTy] = &[<f32 as ScriptType>::TY, <EntityId as ScriptType>::TY];
        assert_eq!(PARAMS, &[NativeTy::Float, NativeTy::Entity]);
    }

    #[test]
    fn an_optional_is_the_inner_type_made_optional() {
        assert_eq!(
            <Option<EntityId> as ScriptType>::TY,
            NativeTy::Optional(&NativeTy::Entity)
        );
    }

    #[test]
    fn an_absent_optional_reads_back_as_none() {
        assert_eq!(<Option<f32>>::from_value(Value::Null), Ok(None));
        assert_eq!(<Option<f32>>::from_value(Value::Float(1.0)), Ok(Some(1.0)));
    }

    /// The language has no implicit narrowing, so a native must not invent one
    /// where the checker refused it.
    #[test]
    fn a_float_does_not_arrive_as_an_int() {
        assert!(i64::from_value(Value::Float(1.9)).is_err());
    }

    /// An integer *does* widen, matching the one implicit conversion the
    /// language allows.
    #[test]
    fn an_int_arrives_as_a_float() {
        assert_eq!(f32::from_value(Value::Int(3)), Ok(3.0));
    }

    /// The message blames the engine, not the author: the checker already
    /// proved the call well-typed, so a mismatch here is a registry that does
    /// not match the program.
    #[test]
    fn a_mismatch_names_the_real_culprit() {
        let error = EntityId::from_value(Value::Int(1)).expect_err("not an entity");
        assert!(error.message.contains("Entity"), "got: {error}");
        assert!(error.message.contains("disagree"), "got: {error}");
    }
}
