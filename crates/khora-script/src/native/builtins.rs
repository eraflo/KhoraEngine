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

//! The functions every Ergon program can call.
//!
//! Scalar maths, and only that for now. Not an arbitrary stopping point: a
//! native can only take and return what a register can hold, and [`Value`]
//! carries numbers, booleans and null. `Log`, `Spawn` and `Raycast` need a
//! string, an entity handle and an engine struct respectively, none of which is
//! representable yet — so they are absent rather than half-present.
//!
//! Each of these is one arithmetic operation, so each costs what a VM
//! instruction costs. A native whose work is not constant — a raycast, an asset
//! load — must declare what it actually spends, or the budget stops meaning
//! anything.

use super::{NativeContext, NativeError, NativeFn, NativeTy};
use crate::vm::Value;

/// The cost of a native that does about as much as one instruction.
const TRIVIAL: u64 = 1;

/// Reads argument `index` as a float, widening an integer.
///
/// The checker has already proved the argument well-typed, so a failure here is
/// an engine bug rather than an author's mistake — which is why the message
/// says so instead of blaming the script.
fn float_at(args: &[Value], index: usize, name: &str) -> Result<f32, NativeError> {
    args.get(index)
        .and_then(|value| value.as_float())
        .ok_or_else(|| {
            NativeError::new(format!(
                "`{name}` was called with a value it cannot use — the argument \
                 did not survive as a number"
            ))
        })
}

macro_rules! unary_float {
    ($konst:ident, $name:literal, $doc:literal, $body:expr) => {
        #[doc = $doc]
        pub static $konst: NativeFn = NativeFn {
            name: $name,
            params: &[NativeTy::Float],
            result: NativeTy::Float,
            cost: TRIVIAL,
            call: |_: &mut NativeContext<'_>, args: &[Value]| {
                let x = float_at(args, 0, $name)?;
                let f: fn(f32) -> f32 = $body;
                Ok(Value::Float(f(x)))
            },
        };
    };
}

macro_rules! binary_float {
    ($konst:ident, $name:literal, $doc:literal, $body:expr) => {
        #[doc = $doc]
        pub static $konst: NativeFn = NativeFn {
            name: $name,
            params: &[NativeTy::Float, NativeTy::Float],
            result: NativeTy::Float,
            cost: TRIVIAL,
            call: |_: &mut NativeContext<'_>, args: &[Value]| {
                let a = float_at(args, 0, $name)?;
                let b = float_at(args, 1, $name)?;
                let f: fn(f32, f32) -> f32 = $body;
                Ok(Value::Float(f(a, b)))
            },
        };
    };
}

unary_float!(ABS, "Abs", "Magnitude, without the sign.", f32::abs);
unary_float!(
    FLOOR,
    "Floor",
    "The largest whole number no greater.",
    f32::floor
);
unary_float!(
    CEIL,
    "Ceil",
    "The smallest whole number no less.",
    f32::ceil
);
unary_float!(ROUND, "Round", "The nearest whole number.", f32::round);
unary_float!(SIGN, "Sign", "`-1`, `0` or `1`.", |x| {
    // `f32::signum` answers 1.0 for +0.0 and -1.0 for -0.0, which reads as a
    // direction where there is none. Gameplay asking for the sign of zero wants
    // "neither way".
    if x == 0.0 {
        0.0
    } else {
        x.signum()
    }
});

binary_float!(MIN, "Min", "The smaller of two.", f32::min);
binary_float!(MAX, "Max", "The larger of two.", f32::max);

/// The non-negative square root.
///
/// Negative input is a fault rather than a `NaN`. A `NaN` propagates silently
/// through every later calculation and surfaces as an entity that has vanished
/// from the world; the fault names the call that produced it.
pub static SQRT: NativeFn = NativeFn {
    name: "Sqrt",
    params: &[NativeTy::Float],
    result: NativeTy::Float,
    cost: TRIVIAL,
    call: |_, args| {
        let x = float_at(args, 0, "Sqrt")?;
        if x < 0.0 {
            return Err(NativeError::new(format!(
                "`Sqrt` cannot take the root of {x}"
            )));
        }
        Ok(Value::Float(x.sqrt()))
    },
};

/// Confines a value to a range.
pub static CLAMP: NativeFn = NativeFn {
    name: "Clamp",
    params: &[NativeTy::Float, NativeTy::Float, NativeTy::Float],
    result: NativeTy::Float,
    cost: TRIVIAL,
    call: |_, args| {
        let value = float_at(args, 0, "Clamp")?;
        let low = float_at(args, 1, "Clamp")?;
        let high = float_at(args, 2, "Clamp")?;
        if low > high {
            return Err(NativeError::new(format!(
                "`Clamp` was given the range {low}..{high}, which is empty"
            )));
        }
        Ok(Value::Float(value.clamp(low, high)))
    },
};

/// Blends between two values.
///
/// Unclamped on purpose: `Lerp(a, b, 1.5)` overshooting is how a spring or an
/// ease-out is written, and clamping here would quietly remove that.
pub static LERP: NativeFn = NativeFn {
    name: "Lerp",
    params: &[NativeTy::Float, NativeTy::Float, NativeTy::Float],
    result: NativeTy::Float,
    cost: TRIVIAL,
    call: |_, args| {
        let from = float_at(args, 0, "Lerp")?;
        let to = float_at(args, 1, "Lerp")?;
        let t = float_at(args, 2, "Lerp")?;
        Ok(Value::Float(from + (to - from) * t))
    },
};

/// Every built-in, in the order they are registered.
///
/// A slice rather than a lazily-built map: the order decides the index compiled
/// bytecode addresses, so it has to be something written down once rather than
/// whatever an iteration produced.
pub fn builtins() -> &'static [&'static NativeFn] {
    BUILTINS
}

static BUILTINS: &[&NativeFn] = &[
    &ABS, &FLOOR, &CEIL, &ROUND, &SIGN, &MIN, &MAX, &SQRT, &CLAMP, &LERP,
];
