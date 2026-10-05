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

//! Arithmetic on engine types, as engine functions.
//!
//! `a + b` on two `Vec3`s is the native named `"Vec3 + Vec3"`, the way `v.x`
//! is the native named `"Vec3.x"`: an engine type's operations are what the
//! engine exposes, not what the instruction set happens to know. The names
//! hold a space, so no script can call one by name — only the lowering of an
//! operator finds it.
//!
//! The table below is the whole rule. The checker types an engine-type
//! operation by looking its native up, and refuses one it does not find, so an
//! operation is defined exactly when it is listed here: `Vec3 * Vec3`, `Quat +
//! Quat` and `Vec3 % float` are not, and a script asking for one is told so
//! where it wrote it.

use super::{NativeError, NativeFn, NativeRegistration, NativeTy};
use crate::vm::Value;

/// The native an operator lowers to: `"Vec3 + Vec3"`, `"float * Color"`.
pub fn binary_operator(left: &str, op: &str, right: &str) -> String {
    format!("{left} {op} {right}")
}

/// The native unary minus lowers to: `"- Vec3"`.
pub fn negation(operand: &str) -> String {
    format!("- {operand}")
}

/// A number operand, whatever width it was written at — an `int` is a
/// `float` here, the language's one implicit widening.
fn scalar(value: &Value, operation: &str) -> Result<f32, NativeError> {
    value
        .as_float()
        .ok_or_else(|| NativeError::new(format!("`{operation}` takes a number")))
}

fn mismatch(operation: &str) -> NativeError {
    NativeError::new(format!("`{operation}` was handed other operands"))
}

/// Registers one operator native.
macro_rules! operator {
    ($name:expr, [$($param:expr),+] -> $result:expr, $call:expr) => {
        const _: () = {
            static OPERATOR: NativeFn = NativeFn {
                name: $name,
                params: &[$($param),+],
                result: $result,
                cost: 1,
                variadic: false,
                call: $call,
            };
            inventory::submit! { NativeRegistration(&OPERATOR) }
        };
    };
}

/// A vector's operators: sum, difference, scaling either side, division by a
/// number, negation.
macro_rules! vector_operators {
    ($variant:ident) => {
        operator!(
            concat!(stringify!($variant), " + ", stringify!($variant)),
            [NativeTy::Engine(stringify!($variant)), NativeTy::Engine(stringify!($variant))]
                -> NativeTy::Engine(stringify!($variant)),
            |_, args| match args {
                [Value::$variant(a), Value::$variant(b)] => Ok(Value::$variant(*a + *b)),
                _ => Err(mismatch(concat!(stringify!($variant), " + ", stringify!($variant)))),
            }
        );
        operator!(
            concat!(stringify!($variant), " - ", stringify!($variant)),
            [NativeTy::Engine(stringify!($variant)), NativeTy::Engine(stringify!($variant))]
                -> NativeTy::Engine(stringify!($variant)),
            |_, args| match args {
                [Value::$variant(a), Value::$variant(b)] => Ok(Value::$variant(*a - *b)),
                _ => Err(mismatch(concat!(stringify!($variant), " - ", stringify!($variant)))),
            }
        );
        operator!(
            concat!(stringify!($variant), " * float"),
            [NativeTy::Engine(stringify!($variant)), NativeTy::Float]
                -> NativeTy::Engine(stringify!($variant)),
            |_, args| match args {
                [Value::$variant(a), s] => {
                    let s = scalar(s, concat!(stringify!($variant), " * float"))?;
                    Ok(Value::$variant(*a * s))
                }
                _ => Err(mismatch(concat!(stringify!($variant), " * float"))),
            }
        );
        operator!(
            concat!("float * ", stringify!($variant)),
            [NativeTy::Float, NativeTy::Engine(stringify!($variant))]
                -> NativeTy::Engine(stringify!($variant)),
            |_, args| match args {
                [s, Value::$variant(a)] => {
                    let s = scalar(s, concat!("float * ", stringify!($variant)))?;
                    Ok(Value::$variant(*a * s))
                }
                _ => Err(mismatch(concat!("float * ", stringify!($variant)))),
            }
        );
        operator!(
            concat!(stringify!($variant), " / float"),
            [NativeTy::Engine(stringify!($variant)), NativeTy::Float]
                -> NativeTy::Engine(stringify!($variant)),
            |_, args| match args {
                [Value::$variant(a), s] => {
                    let s = scalar(s, concat!(stringify!($variant), " / float"))?;
                    Ok(Value::$variant(*a / s))
                }
                _ => Err(mismatch(concat!(stringify!($variant), " / float"))),
            }
        );
        operator!(
            concat!("- ", stringify!($variant)),
            [NativeTy::Engine(stringify!($variant))] -> NativeTy::Engine(stringify!($variant)),
            |_, args| match args {
                [Value::$variant(a)] => Ok(Value::$variant(-*a)),
                _ => Err(mismatch(concat!("- ", stringify!($variant)))),
            }
        );
    };
}

vector_operators!(Vec2);
vector_operators!(Vec3);
vector_operators!(Vec4);

// A rotation composes with a rotation and turns a vector; it is not a sum.
operator!(
    "Quat * Quat",
    [NativeTy::Engine("Quat"), NativeTy::Engine("Quat")] -> NativeTy::Engine("Quat"),
    |_, args| match args {
        [Value::Quat(a), Value::Quat(b)] => Ok(Value::Quat(*a * *b)),
        _ => Err(mismatch("Quat * Quat")),
    }
);
operator!(
    "Quat * Vec3",
    [NativeTy::Engine("Quat"), NativeTy::Engine("Vec3")] -> NativeTy::Engine("Vec3"),
    |_, args| match args {
        [Value::Quat(q), Value::Vec3(v)] => Ok(Value::Vec3(*q * *v)),
        _ => Err(mismatch("Quat * Vec3")),
    }
);

// A colour adds, subtracts, modulates by a colour and scales by a number.
operator!(
    "Color + Color",
    [NativeTy::Engine("Color"), NativeTy::Engine("Color")] -> NativeTy::Engine("Color"),
    |_, args| match args {
        [Value::Color(a), Value::Color(b)] => Ok(Value::Color(*a + *b)),
        _ => Err(mismatch("Color + Color")),
    }
);
operator!(
    "Color - Color",
    [NativeTy::Engine("Color"), NativeTy::Engine("Color")] -> NativeTy::Engine("Color"),
    |_, args| match args {
        [Value::Color(a), Value::Color(b)] => Ok(Value::Color(*a - *b)),
        _ => Err(mismatch("Color - Color")),
    }
);
operator!(
    "Color * Color",
    [NativeTy::Engine("Color"), NativeTy::Engine("Color")] -> NativeTy::Engine("Color"),
    |_, args| match args {
        [Value::Color(a), Value::Color(b)] => Ok(Value::Color(*a * *b)),
        _ => Err(mismatch("Color * Color")),
    }
);
operator!(
    "Color * float",
    [NativeTy::Engine("Color"), NativeTy::Float] -> NativeTy::Engine("Color"),
    |_, args| match args {
        [Value::Color(a), s] => Ok(Value::Color(*a * scalar(s, "Color * float")?)),
        _ => Err(mismatch("Color * float")),
    }
);
operator!(
    "float * Color",
    [NativeTy::Float, NativeTy::Engine("Color")] -> NativeTy::Engine("Color"),
    |_, args| match args {
        [s, Value::Color(a)] => Ok(Value::Color(*a * scalar(s, "float * Color")?)),
        _ => Err(mismatch("float * Color")),
    }
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_operator_is_named_by_its_operands() {
        assert_eq!(binary_operator("Vec3", "+", "Vec3"), "Vec3 + Vec3");
        assert_eq!(binary_operator("float", "*", "Color"), "float * Color");
        assert_eq!(negation("Vec2"), "- Vec2");
    }
}
