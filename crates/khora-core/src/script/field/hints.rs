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

//! How the inspector presents one field of a script-declared component:
//! `[Range(min, max)]`, `[Tooltip("…")]` and `[Header("…")]` as written.

use serde::{Deserialize, Serialize};

/// How the inspector presents one field of a script-declared component.
/// Editor-only: nothing at run time reads it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct FieldHints {
    /// `[Range(min, max)]`, in the field's stored unit: seconds for a
    /// `Duration`, radians for an `Angle`.
    pub range: Option<FieldRange>,
    /// `[Tooltip("…")]`: what hovering the field's label shows.
    pub tooltip: Option<String>,
    /// `[Header("…")]`: the section title shown above the field.
    pub header: Option<String>,
}

impl FieldHints {
    /// Whether no hint is given: no range, no tooltip, no header.
    pub fn is_empty(&self) -> bool {
        self.range.is_none() && self.tooltip.is_none() && self.header.is_none()
    }
}

/// The bounds `[Range(min, max)]` declares, inclusive at both ends.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FieldRange {
    /// The lowest value the inspector edits to.
    pub min: f64,
    /// The highest value the inspector edits to.
    pub max: f64,
}

impl FieldRange {
    /// Whether `value` lies within the bounds, both included.
    pub fn contains(&self, value: f64) -> bool {
        self.min <= value && value <= self.max
    }

    /// Whether `value`, a 32-bit float, lies within the bounds — one that
    /// converting a unit left a step or two of a 32-bit float past a bound
    /// counting as at it: `3000ms` is `3.0000002` seconds, and lies within
    /// `0s..=3s`.
    pub fn holds(&self, value: f64) -> bool {
        let near = |bound: f64| steps_apart(value as f32, bound as f32) <= 2;
        self.contains(value) || near(self.min) || near(self.max)
    }

    /// `value` brought within the bounds.
    pub fn clamp(&self, value: f64) -> f64 {
        // Never `f64::clamp`, which panics on bounds read from a damaged
        // file.
        value.max(self.min).min(self.max)
    }

    /// Where `value` sits between the bounds: `0` at `min`, `1` at `max`,
    /// clamped to `0..=1` outside them.
    pub fn fraction(&self, value: f64) -> f64 {
        let span = self.max - self.min;
        if span <= 0.0 {
            return 0.0;
        }
        ((value - self.min) / span).clamp(0.0, 1.0)
    }
}

/// How many 32-bit floats lie from `a` to `b`, counting `b`; `u64::MAX` when
/// either is not a number.
fn steps_apart(a: f32, b: f32) -> u64 {
    if a.is_nan() || b.is_nan() {
        return u64::MAX;
    }
    // The bits of a float, ordered as the floats are: negatives below zero.
    let ordered = |x: f32| {
        let bits = x.to_bits() as i32;
        i64::from(if bits < 0 { i32::MIN - bits } else { bits })
    };
    ordered(a).abs_diff(ordered(b))
}
