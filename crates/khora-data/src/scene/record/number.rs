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

//! Numbers read at the width a field asks for, when that loses nothing.
//!
//! A save may hold a number at another width than the field now reading it —
//! a count that became a float, a float that became a double. Widening is
//! lossless and allowed; anything that would round, truncate or saturate is
//! refused, because a value that changes on load is data lost without a word.

use super::Record;

/// What a number record holds, whatever width it was written at.
#[derive(Clone, Copy)]
pub(super) enum Number {
    Signed(i64),
    Unsigned(u64),
    Single(f32),
    Double(f64),
}

impl Number {
    pub(super) fn of(record: &Record) -> Option<Self> {
        match *record {
            Record::I64(v) => Some(Self::Signed(v)),
            Record::U64(v) => Some(Self::Unsigned(v)),
            Record::F32(v) => Some(Self::Single(v)),
            Record::F64(v) => Some(Self::Double(v)),
            _ => None,
        }
    }

    pub(super) fn is_float(self) -> bool {
        matches!(self, Self::Single(_) | Self::Double(_))
    }

    /// As an exact integer, if it is one. Floats qualify only when integral:
    /// a fraction dropped on the way to an integer field is data lost.
    pub(super) fn integer(self) -> Option<i128> {
        let float = match self {
            Self::Signed(v) => return Some(v.into()),
            Self::Unsigned(v) => return Some(v.into()),
            Self::Single(v) => f64::from(v),
            Self::Double(v) => v,
        };
        // 2^64 bounds every integer field; beyond it `as` would saturate and
        // pass a wrong value off as exact.
        const LIMIT: f64 = 18_446_744_073_709_551_616.0;
        (float.is_finite() && float.fract() == 0.0 && float.abs() < LIMIT).then_some(float as i128)
    }

    /// As an `f64`, if that loses nothing.
    pub(super) fn double(self) -> Option<f64> {
        match self {
            Self::Single(v) => Some(f64::from(v)),
            Self::Double(v) => Some(v),
            Self::Signed(v) => exact_integer(v.into(), v as f64),
            Self::Unsigned(v) => exact_integer(v.into(), v as f64),
        }
    }

    /// As an `f32`, if that loses nothing.
    pub(super) fn single(self) -> Option<f32> {
        match self {
            Self::Single(v) => Some(v),
            Self::Double(v) => {
                let narrowed = v as f32;
                (f64::from(narrowed) == v || v.is_nan()).then_some(narrowed)
            }
            Self::Signed(v) => {
                let narrowed = v as f32;
                exact_integer(v.into(), f64::from(narrowed)).map(|_| narrowed)
            }
            Self::Unsigned(v) => {
                let narrowed = v as f32;
                exact_integer(v.into(), f64::from(narrowed)).map(|_| narrowed)
            }
        }
    }
}

/// `float` when it is exactly the integer `exact`.
///
/// Compared through `i128`, which holds every `i64` and `u64`: comparing in
/// `f64`, or casting back with `as`, would call `i64::MAX` exactly
/// representable because both sides round the same way.
fn exact_integer(exact: i128, float: f64) -> Option<f64> {
    let back = Number::Double(float).integer()?;
    (back == exact).then_some(float)
}
