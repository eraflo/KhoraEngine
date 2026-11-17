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

//! What a Rust type means to a script: the trait `#[ergon_fn]` compiles
//! against, so the compiler resolves the mapping rather than the macro reading
//! a type's written name.

use khora_core::ecs::entity::EntityId;

use super::{NativeContext, NativeError, NativeTy};
use crate::vm::Value;

/// A Rust type a script can pass or receive.
pub trait ScriptType: Sized {
    /// How the type is written in Ergon.
    ///
    /// A constant, so a generated signature is a `static`.
    const TY: NativeTy;

    /// Reads a value the VM produced.
    ///
    /// # Errors
    ///
    /// A value of another shape: the checker proved the call well-typed, so the
    /// registry and the program disagree, and the message says so.
    fn from_value(value: Value, context: &NativeContext<'_>) -> Result<Self, NativeError>;

    /// Turns a result into a value the VM can hold.
    ///
    /// A type too large for a register is put in the context's frame arena.
    ///
    /// # Errors
    ///
    /// The frame arena refused the allocation.
    fn to_value(self, context: &mut NativeContext<'_>) -> Result<Value, NativeError>;
}

/// Builds the message for a value that arrived in the wrong shape.
fn mismatch(expected: &str, found: Value) -> NativeError {
    NativeError::new(format!(
        "an engine function expected {expected} but the call supplied {} — \
         the program and the engine disagree about its signature",
        found.type_name()
    ))
}

impl ScriptType for f32 {
    const TY: NativeTy = NativeTy::Float;

    fn from_value(value: Value, _: &NativeContext<'_>) -> Result<Self, NativeError> {
        value.as_float().ok_or_else(|| mismatch("a float", value))
    }

    fn to_value(self, _: &mut NativeContext<'_>) -> Result<Value, NativeError> {
        Ok(Value::Float(self))
    }
}

impl ScriptType for i64 {
    const TY: NativeTy = NativeTy::Int;

    fn from_value(value: Value, _: &NativeContext<'_>) -> Result<Self, NativeError> {
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

    fn from_value(value: Value, _: &NativeContext<'_>) -> Result<Self, NativeError> {
        value.as_bool().ok_or_else(|| mismatch("a bool", value))
    }

    fn to_value(self, _: &mut NativeContext<'_>) -> Result<Value, NativeError> {
        Ok(Value::Bool(self))
    }
}

impl ScriptType for EntityId {
    const TY: NativeTy = NativeTy::Entity;

    fn from_value(value: Value, _: &NativeContext<'_>) -> Result<Self, NativeError> {
        value
            .as_entity()
            .ok_or_else(|| mismatch("an Entity", value))
    }

    fn to_value(self, _: &mut NativeContext<'_>) -> Result<Value, NativeError> {
        Ok(Value::Entity(self))
    }
}

/// Text handed to or from an engine function.
///
/// Owned: a borrow would name the program's lifetime or the arena's depending
/// on where the string lives. Reading one copies it; returning one allocates
/// in the frame arena, so it lasts one frame.
impl ScriptType for String {
    const TY: NativeTy = NativeTy::Str;

    fn from_value(value: Value, context: &NativeContext<'_>) -> Result<Self, NativeError> {
        context.string(value).map(str::to_owned)
    }

    fn to_value(self, context: &mut NativeContext<'_>) -> Result<Value, NativeError> {
        let reference = context
            .arena
            .alloc(crate::arena::Object::Str(self))
            .map_err(|error| NativeError::new(error.message()))?;
        Ok(Value::Str(crate::vm::StrRef::Arena(reference)))
    }
}

impl ScriptType for () {
    const TY: NativeTy = NativeTy::Void;

    fn from_value(_: Value, _: &NativeContext<'_>) -> Result<Self, NativeError> {
        Ok(())
    }

    fn to_value(self, _: &mut NativeContext<'_>) -> Result<Value, NativeError> {
        Ok(Value::Unit)
    }
}

/// `T?` — an absent value is `null` rather than a missing argument.
impl<T: ScriptType> ScriptType for Option<T> {
    const TY: NativeTy = NativeTy::Optional(&T::TY);

    fn from_value(value: Value, context: &NativeContext<'_>) -> Result<Self, NativeError> {
        if value.is_null() {
            return Ok(None);
        }
        T::from_value(value, context).map(Some)
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

    /// A host and a program's literals, so a test can build a context.
    struct Fixture {
        host: crate::native::Host,
        strings: Vec<String>,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                host: crate::native::Host::new(),
                strings: Vec::new(),
            }
        }

        fn with_literal(text: &str) -> Self {
            Self {
                host: crate::native::Host::new(),
                strings: vec![text.to_owned()],
            }
        }

        fn context(&mut self) -> NativeContext<'_> {
            self.host.context(&self.strings)
        }
    }

    fn round_trip<T: ScriptType + PartialEq + std::fmt::Debug + Clone>(value: T) {
        let mut fixture = Fixture::new();
        let mut context = fixture.context();

        let encoded = value.clone().to_value(&mut context).expect("encodes");
        assert_eq!(T::from_value(encoded, &context).expect("decodes"), value);
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

    /// Text returned by a native lands in the frame arena, and reads back from
    /// there — the round trip crosses the arena rather than skipping it.
    #[test]
    fn a_string_round_trips_through_the_arena() {
        round_trip("héllo".to_owned());
    }

    /// A literal resolves from the program, so a native reads it without the
    /// arena being involved at all.
    #[test]
    fn a_literal_resolves_from_the_program() {
        let mut fixture = Fixture::with_literal("hit");
        let context = fixture.context();

        let value = Value::Str(crate::vm::StrRef::Const(0));
        assert_eq!(String::from_value(value, &context).as_deref(), Ok("hit"));
    }

    /// A constant index the program does not have is refused rather than
    /// answering with a neighbouring literal.
    #[test]
    fn a_literal_from_another_program_is_refused() {
        let mut fixture = Fixture::with_literal("hit");
        let context = fixture.context();

        let value = Value::Str(crate::vm::StrRef::Const(9));
        assert!(String::from_value(value, &context).is_err());
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
        let mut fixture = Fixture::new();
        let context = fixture.context();

        assert_eq!(<Option<f32>>::from_value(Value::Null, &context), Ok(None));
        assert_eq!(
            <Option<f32>>::from_value(Value::Float(1.0), &context),
            Ok(Some(1.0))
        );
    }

    /// The language has no implicit narrowing, so a native must not invent one
    /// where the checker refused it.
    #[test]
    fn a_float_does_not_arrive_as_an_int() {
        let mut fixture = Fixture::new();
        let context = fixture.context();
        assert!(i64::from_value(Value::Float(1.9), &context).is_err());
    }

    /// An integer *does* widen, matching the one implicit conversion the
    /// language allows.
    #[test]
    fn an_int_arrives_as_a_float() {
        let mut fixture = Fixture::new();
        let context = fixture.context();
        assert_eq!(f32::from_value(Value::Int(3), &context), Ok(3.0));
    }

    /// The message blames the engine, not the author.
    ///
    /// The checker proved the call well-typed: a mismatch is a registry that
    /// does not match the program.
    #[test]
    fn a_mismatch_names_the_real_culprit() {
        let mut fixture = Fixture::new();
        let context = fixture.context();

        let error = EntityId::from_value(Value::Int(1), &context).expect_err("not an entity");
        assert!(error.message.contains("Entity"), "got: {error}");
        assert!(error.message.contains("disagree"), "got: {error}");
    }
}
