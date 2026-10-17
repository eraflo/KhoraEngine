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

//! What a field's hints say on its row, without drawing it: the section
//! header above it, its range in the units the inspector shows, whether the
//! value it holds lies outside that range and why, how fast its scrubber
//! drags, how an edit is kept inside, and where its gauge fills to.
//!
//! A range constrains the inspector only: a value a script or a file wrote
//! outside it is shown as it is, and changes only when the author edits it.

use khora_sdk::editor_ui::DeclaredFieldView;
use khora_sdk::khora_core::script::{ErgonType, FieldRange, ScriptValue};

/// The section header shown above `field`, as its `[Header("…")]` gives
/// it — `None` when it opens no section.
pub fn section_header(field: &DeclaredFieldView) -> Option<String> {
    field.hints.header.clone()
}

/// `range`'s bounds as the inspector shows a `ty` value: `0–100` for an
/// `int`, `0.5–2` for a `float`, `0–5 s` for a `Duration`, `0–90 deg` for
/// an `Angle` stored in radians.
pub fn range_text(ty: &ErgonType, range: FieldRange) -> String {
    let (min, max) = (shown(ty, range.min), shown(ty, range.max));
    match ty {
        ErgonType::Duration => format!("{min}–{max} s"),
        ErgonType::Angle => format!("{min}–{max} deg"),
        _ => format!("{min}–{max}"),
    }
}

/// A bound as the inspector shows a `ty` value: whole for an `int`, in
/// degrees for an angle, else as short as the float it was written as.
fn shown(ty: &ErgonType, value: f64) -> String {
    match ty {
        ErgonType::Int => format!("{}", value as i64),
        // Radians back to degrees round off in the last digits: a thousandth
        // of a degree is finer than any scrubber edits.
        ErgonType::Angle => format!(
            "{}",
            ((value as f32).to_degrees() * 1000.0).round() / 1000.0
        ),
        _ => format!("{}", value as f32),
    }
}

/// Whether `value`, a numeric leaf of type `ty`, lies outside `range`. A
/// value that is not a number of `ty` is never outside.
pub fn outside_range(ty: &ErgonType, value: &ScriptValue, range: FieldRange) -> bool {
    match (ty, value) {
        (ErgonType::Int, ScriptValue::Int(i)) => !range.contains(*i as f64),
        (ErgonType::Float | ErgonType::Duration | ErgonType::Angle, ScriptValue::Float(f)) => {
            !range.holds(f64::from(*f))
        }
        _ => false,
    }
}

/// What hovering a value outside `range` says — `None` for one inside.
pub fn range_note(ty: &ErgonType, value: &ScriptValue, range: FieldRange) -> Option<String> {
    outside_range(ty, value, range).then(|| {
        format!(
            "Outside {}: a script or a file wrote it. Editing brings it back inside.",
            range_text(ty, range)
        )
    })
}

/// How far a `ty` scrubber bounded by `range` moves per point dragged, in
/// the units it edits (degrees for an angle): about 200 points span the
/// range.
pub fn ranged_speed(ty: &ErgonType, range: FieldRange) -> f32 {
    // The span the scrubber edits — an angle is dragged in degrees.
    let span = match ty {
        ErgonType::Angle => (range.max - range.min).to_degrees(),
        _ => range.max - range.min,
    };
    let speed = (span / SCRUB_POINTS) as f32;
    if speed.is_finite() && speed > 0.0 {
        speed
    } else {
        unbounded_speed(ty)
    }
}

/// How many points of drag span a range.
const SCRUB_POINTS: f64 = 200.0;

/// How far an unbounded `ty` scrubber moves per point dragged.
pub fn unbounded_speed(ty: &ErgonType) -> f32 {
    match ty {
        ErgonType::Int => 0.25,
        ErgonType::Duration => 0.01,
        ErgonType::Angle => 0.5,
        _ => 0.05,
    }
}

/// The value an edit from `before` to `after` stores: `after` kept inside
/// `range` — and `before` untouched when nothing was edited.
pub fn edit_within(before: f64, after: f64, range: FieldRange) -> f64 {
    if after == before {
        before
    } else {
        range.clamp(after)
    }
}

/// How far `value` fills the gauge under its scrubber: `0` at `range.min`,
/// `1` at `range.max`, held within `0..=1`.
pub fn gauge_fraction(value: f64, range: FieldRange) -> f32 {
    let fraction = range.fraction(value) as f32;
    if fraction.is_nan() {
        0.0
    } else {
        fraction.clamp(0.0, 1.0)
    }
}
