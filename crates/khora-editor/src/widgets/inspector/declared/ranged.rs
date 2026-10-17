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

//! A number on a declared card — an `int`, a `float`, a `Duration` in
//! seconds, an `Angle` in degrees — and what its hints add around it: the
//! section header above the field, the tooltip on its label, and for a
//! ranged one a scrubber kept inside the range with a gauge under it.
//!
//! A value outside the range is never changed by being shown: its gauge
//! turns amber and its scrubber says why, until the author edits it.

use khora_sdk::editor_ui::{UiBuilder, UiTheme};
use khora_sdk::khora_core::script::{ErgonType, FieldRange, ScriptValue};
use khora_tool_ui::widgets::paint::eyebrow;

use super::fields::FieldContext;
use super::hints::{
    edit_within, gauge_fraction, outside_range, range_note, ranged_speed, unbounded_speed,
};

/// The height of a gauge row: the 2-point bar and the gap above it.
const GAUGE_ROOM: f32 = 4.0;
/// The gauge's thickness.
const GAUGE_THICKNESS: f32 = 2.0;
/// Room above a header opening the card's first field, and any other.
const HEADER_ROOM_FIRST: f32 = 16.0;
const HEADER_ROOM: f32 = 26.0;
/// How far above the bottom of its room a header's text starts.
const HEADER_LIFT: f32 = 13.0;

/// Draws the section header `text` above a field: more room above it than
/// below, less as the card's first row.
pub fn render_header(ui: &mut dyn UiBuilder, theme: &UiTheme, text: &str, first: bool) {
    let room = if first {
        HEADER_ROOM_FIRST
    } else {
        HEADER_ROOM
    };
    let width = ui.available_width();
    let rect = ui.allocate_size([width, room]);
    eyebrow(ui, theme, [rect[0], rect[1] + room - HEADER_LIFT], text);
}

/// The field's label, saying its tooltip when hovered.
pub fn render_label(ui: &mut dyn UiBuilder, cx: &FieldContext<'_>, text: &str, color: [f32; 4]) {
    match cx.tooltip {
        Some(tooltip) => ui.colored_label_with_tooltip(color, text, tooltip),
        None => ui.colored_label(color, text),
    }
}

/// Draws `value`, a number of `ty`, on one row — its label, its scrubber,
/// its unit — and its gauge under it when ranged. Returns whether the author
/// changed it; `None` when `value` is not a number of `ty`.
pub fn render_number(
    ui: &mut dyn UiBuilder,
    label: &str,
    ty: &ErgonType,
    value: &mut ScriptValue,
    cx: &FieldContext<'_>,
) -> Option<bool> {
    // What the scrubber edits: an angle in degrees, all else as stored.
    let (mut edited, unit) = match (ty, &*value) {
        (ErgonType::Int, ScriptValue::Int(i)) => (*i as f32, None),
        (ErgonType::Float, ScriptValue::Float(f)) => (*f, None),
        (ErgonType::Duration, ScriptValue::Float(seconds)) => (*seconds, Some("s")),
        (ErgonType::Angle, ScriptValue::Float(radians)) => (radians.to_degrees(), Some("deg")),
        _ => return None,
    };
    let before = edited;
    let note = cx.range.and_then(|range| range_note(ty, value, range));
    let mut changed = false;
    ui.horizontal(&mut |row| {
        render_label(row, cx, label, cx.theme.text);
        changed = match cx.range {
            Some(range) => {
                let (min, max) = edited_bounds(ty, range);
                row.drag_value_f32_in("", &mut edited, ranged_speed(ty, range), min, max)
            }
            None => row.drag_value_f32("", &mut edited, unbounded_speed(ty)),
        };
        if let Some(note) = &note {
            row.tooltip_for_last(note);
        }
        if let Some(unit) = unit {
            row.colored_label(cx.theme.text_muted, unit);
        }
    });
    if changed {
        if let Some(range) = cx.range {
            let (min, max) = edited_bounds(ty, range);
            let bounds = FieldRange {
                min: f64::from(min),
                max: f64::from(max),
            };
            edited = edit_within(f64::from(before), f64::from(edited), bounds) as f32;
        }
        match value {
            ScriptValue::Int(i) => *i = edited.round() as i64,
            ScriptValue::Float(f) if *ty == ErgonType::Angle => *f = edited.to_radians(),
            ScriptValue::Float(f) => *f = edited,
            _ => {}
        }
    }
    if let Some(range) = cx.range {
        render_gauge(ui, cx.theme, ty, value, range);
    }
    Some(changed)
}

/// A read-only number's gauge, under its text row.
pub fn render_read_only_gauge(
    ui: &mut dyn UiBuilder,
    cx: &FieldContext<'_>,
    ty: &ErgonType,
    value: &ScriptValue,
) {
    if let Some(range) = cx.range {
        if numeric(value).is_some() {
            render_gauge(ui, cx.theme, ty, value, range);
        }
    }
}

/// The bar under a ranged number: a quiet track, filled to where the value
/// sits — all amber when it lies outside the range.
fn render_gauge(
    ui: &mut dyn UiBuilder,
    theme: &UiTheme,
    ty: &ErgonType,
    value: &ScriptValue,
    range: FieldRange,
) {
    let Some(number) = numeric(value) else {
        return;
    };
    let width = ui.available_width();
    let rect = ui.allocate_size([width, GAUGE_ROOM]);
    let top = rect[1] + rect[3] - GAUGE_THICKNESS;
    let radius = GAUGE_THICKNESS / 2.0;
    if outside_range(ty, value, range) {
        ui.paint_rect_filled(
            [rect[0], top],
            [rect[2], GAUGE_THICKNESS],
            theme.warning,
            radius,
        );
        return;
    }
    ui.paint_rect_filled(
        [rect[0], top],
        [rect[2], GAUGE_THICKNESS],
        theme.separator,
        radius,
    );
    let filled = rect[2] * gauge_fraction(number, range);
    if filled > 0.0 {
        ui.paint_rect_filled(
            [rect[0], top],
            [filled, GAUGE_THICKNESS],
            theme.text_dim,
            radius,
        );
    }
}

/// The bounds a `ty` scrubber edits between — an angle's in degrees.
fn edited_bounds(ty: &ErgonType, range: FieldRange) -> (f32, f32) {
    match ty {
        ErgonType::Angle => (
            (range.min as f32).to_degrees(),
            (range.max as f32).to_degrees(),
        ),
        _ => (range.min as f32, range.max as f32),
    }
}

/// The number `value` holds, as stored.
fn numeric(value: &ScriptValue) -> Option<f64> {
    match value {
        ScriptValue::Int(i) => Some(*i as f64),
        ScriptValue::Float(f) => Some(f64::from(*f)),
        _ => None,
    }
}
