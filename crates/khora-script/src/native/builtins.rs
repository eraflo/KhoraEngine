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
//! Written with `#[ergon_fn]`, the same attribute a game uses. The language's
//! own built-ins are therefore the macro's first consumer: a signature it gets
//! wrong breaks `Abs` before it breaks anyone's project.
//!
//! Scalar maths and text, which is what a register can hold today. `Spawn` and
//! `Raycast` need an engine struct, so they are absent rather than
//! half-present.
//!
//! Each of these does about as much as one instruction and is priced that way.
//! A native whose work is not constant — a raycast, an asset load — must
//! declare what it actually spends, or the budget stops meaning anything.

use khora_macros::ergon_fn;

use super::{NativeError, NativeFn};

// ─── Numbers ────────────────────────────────────────────────────────────────

/// Magnitude, without the sign.
#[ergon_fn]
fn abs(x: f32) -> f32 {
    x.abs()
}

/// The largest whole number no greater.
#[ergon_fn]
fn floor(x: f32) -> f32 {
    x.floor()
}

/// The smallest whole number no less.
#[ergon_fn]
fn ceil(x: f32) -> f32 {
    x.ceil()
}

/// The nearest whole number.
#[ergon_fn]
fn round(x: f32) -> f32 {
    x.round()
}

/// `-1`, `0` or `1`.
#[ergon_fn]
fn sign(x: f32) -> f32 {
    // `f32::signum` answers 1.0 for +0.0 and -1.0 for -0.0, which reads as a
    // direction where there is none. Gameplay asking for the sign of zero wants
    // "neither way".
    if x == 0.0 {
        0.0
    } else {
        x.signum()
    }
}

/// The smaller of two.
#[ergon_fn]
fn min(a: f32, b: f32) -> f32 {
    a.min(b)
}

/// The larger of two.
#[ergon_fn]
fn max(a: f32, b: f32) -> f32 {
    a.max(b)
}

/// The non-negative square root.
///
/// Negative input is a fault rather than a `NaN`. A `NaN` propagates silently
/// through every later calculation and surfaces as an entity that has vanished
/// from the world; the fault names the call that produced it.
#[ergon_fn]
fn sqrt(x: f32) -> Result<f32, NativeError> {
    if x < 0.0 {
        return Err(NativeError::new(format!(
            "`Sqrt` cannot take the root of {x}"
        )));
    }
    Ok(x.sqrt())
}

/// Confines a value to a range.
#[ergon_fn]
fn clamp(value: f32, low: f32, high: f32) -> Result<f32, NativeError> {
    if low > high {
        return Err(NativeError::new(format!(
            "`Clamp` was given the range {low}..{high}, which is empty"
        )));
    }
    Ok(value.clamp(low, high))
}

/// Blends between two values.
///
/// Unclamped on purpose: `Lerp(a, b, 1.5)` overshooting is how a spring or an
/// ease-out is written, and clamping here would quietly remove that.
#[ergon_fn]
fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

// ─── Text ───────────────────────────────────────────────────────────────────

/// Reports to the editor's Console.
///
/// Through `log::*` rather than any console of its own, so a script's output
/// lands wherever the engine's already does — the editor panel while playing,
/// the terminal for a headless run, a file if the host configured one. A
/// separate channel would have to be plumbed to each of those again.
#[ergon_fn]
fn log(message: String) {
    log::info!("[script] {message}");
}

/// Reports something the author should look at.
#[ergon_fn]
fn warn(message: String) {
    log::warn!("[script] {message}");
}

/// Reports something that went wrong.
///
/// Does **not** stop the behavior. A script reporting a problem it handled is
/// not the same as one that faulted, and conflating them would make the second
/// impossible to see.
#[ergon_fn]
fn error(message: String) {
    log::error!("[script] {message}");
}

/// How many characters a string holds.
///
/// Characters, not bytes: it is what an author can count, and what the arena's
/// own length already reports.
#[ergon_fn]
fn length(text: String) -> i64 {
    text.chars().count() as i64
}

/// Every built-in, in the order they are registered.
///
/// A written list rather than the `inventory` submissions these also make: the
/// order decides the index compiled bytecode addresses, and the built-ins are
/// the part of the registry that must not move when a game adds a function of
/// its own.
pub fn builtins() -> &'static [&'static NativeFn] {
    BUILTINS
}

static BUILTINS: &[&NativeFn] = &[
    &ERGON_ABS,
    &ERGON_FLOOR,
    &ERGON_CEIL,
    &ERGON_ROUND,
    &ERGON_SIGN,
    &ERGON_MIN,
    &ERGON_MAX,
    &ERGON_SQRT,
    &ERGON_CLAMP,
    &ERGON_LERP,
    &ERGON_LOG,
    &ERGON_WARN,
    &ERGON_ERROR,
    &ERGON_LENGTH,
];
