// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! The inspector's leaf renderers, each picked by its JSON shape in
//! `walker::render_object`, and the bridge between JSON and floats.

use khora_sdk::editor_ui::{UiBuilder, UiTheme};
use serde_json::Value;

// ─── shape detection ────────────────────────────────────────────────

/// Returns whether `map` is a vector: numeric `x`, `y`, `z` and nothing else.
pub fn is_vec3(map: &serde_json::Map<String, Value>) -> bool {
    map.len() == 3
        && map.get("x").is_some_and(|v| v.is_number())
        && map.get("y").is_some_and(|v| v.is_number())
        && map.get("z").is_some_and(|v| v.is_number())
}

/// Returns whether `map` is a quaternion: numeric `x`, `y`, `z`, `w` and
/// nothing else.
pub fn is_quat(map: &serde_json::Map<String, Value>) -> bool {
    map.len() == 4
        && map.get("x").is_some_and(|v| v.is_number())
        && map.get("y").is_some_and(|v| v.is_number())
        && map.get("z").is_some_and(|v| v.is_number())
        && map.get("w").is_some_and(|v| v.is_number())
}

/// Returns whether `map` is a colour: numeric `r`, `g`, `b`, `a` and nothing
/// else.
pub fn is_color(map: &serde_json::Map<String, Value>) -> bool {
    map.len() == 4
        && map.get("r").is_some_and(|v| v.is_number())
        && map.get("g").is_some_and(|v| v.is_number())
        && map.get("b").is_some_and(|v| v.is_number())
        && map.get("a").is_some_and(|v| v.is_number())
}

// ─── leaf widgets ───────────────────────────────────────────────────

/// Renders a vector as a drag row; returns whether it changed.
pub fn render_vec3(
    ui: &mut dyn UiBuilder,
    map: &mut serde_json::Map<String, Value>,
    _theme: &UiTheme,
) -> bool {
    let mut v = [
        json_to_f32(map.get("x")),
        json_to_f32(map.get("y")),
        json_to_f32(map.get("z")),
    ];
    let changed = ui.vec3_editor("", &mut v, 0.05);
    if changed {
        map.insert("x".into(), f32_to_json(v[0]));
        map.insert("y".into(), f32_to_json(v[1]));
        map.insert("z".into(), f32_to_json(v[2]));
    }
    changed
}

/// Renders a quaternion as four drag values; returns whether one changed.
pub fn render_quat(ui: &mut dyn UiBuilder, map: &mut serde_json::Map<String, Value>) -> bool {
    let mut changed = false;
    for axis in ["x", "y", "z", "w"] {
        let mut local = json_to_f32(map.get(axis));
        if ui.drag_value_f32(axis, &mut local, 0.01) {
            map.insert(axis.into(), f32_to_json(local));
            changed = true;
        }
    }
    changed
}

/// Renders a colour picker; returns whether the colour changed.
pub fn render_color(
    ui: &mut dyn UiBuilder,
    map: &mut serde_json::Map<String, Value>,
    _theme: &UiTheme,
) -> bool {
    let mut rgba = [
        json_to_f32(map.get("r")),
        json_to_f32(map.get("g")),
        json_to_f32(map.get("b")),
        json_to_f32(map.get("a")),
    ];
    let changed = ui.color_edit("", &mut rgba);
    if changed {
        map.insert("r".into(), f32_to_json(rgba[0]));
        map.insert("g".into(), f32_to_json(rgba[1]));
        map.insert("b".into(), f32_to_json(rgba[2]));
        map.insert("a".into(), f32_to_json(rgba[3]));
    }
    changed
}

/// Renders a number as a drag row, in whole steps for a JSON integer; returns
/// whether it changed.
pub fn render_number_row(ui: &mut dyn UiBuilder, label: &str, value: &mut Value) -> bool {
    // An integer field (`u8`, `u32`, `i64` …) reads back only as a JSON
    // integer: it is dragged in whole steps and written whole.
    if let Some(whole) = integer_of(value) {
        let mut local = whole as f32;
        let changed = ui.drag_value_f32(label, &mut local, 0.5);
        if changed {
            *value = integer_json(local);
        }
        return changed;
    }
    let mut local = json_to_f32(Some(&*value));
    let changed = ui.drag_value_f32(label, &mut local, 0.05);
    if changed {
        *value = f32_to_json(local);
    }
    changed
}

/// The integer `value` holds, when it is a JSON integer rather than a float.
fn integer_of(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) if number.is_u64() || number.is_i64() => number
            .as_i64()
            .or_else(|| number.as_u64().map(|n| n.min(i64::MAX as u64) as i64)),
        _ => None,
    }
}

/// Returns `dragged`, rounded, as a JSON integer.
///
/// Nothing is clamped, since JSON cannot tell an unsigned field from a signed
/// one: a value the field cannot hold (300 for a `u8`) is refused when the
/// component reads the edit back, and the field keeps its last value.
fn integer_json(dragged: f32) -> Value {
    Value::from(dragged.round() as i64)
}

/// Renders a three-number array as a vector row; returns whether it changed.
pub fn render_numeric_triple(ui: &mut dyn UiBuilder, label: &str, arr: &mut [Value]) -> bool {
    let mut v = [
        json_to_f32(Some(&arr[0])),
        json_to_f32(Some(&arr[1])),
        json_to_f32(Some(&arr[2])),
    ];
    let changed = ui.vec3_editor(label, &mut v, 0.05);
    if changed {
        arr[0] = f32_to_json(v[0]);
        arr[1] = f32_to_json(v[1]);
        arr[2] = f32_to_json(v[2]);
    }
    changed
}

/// Renders a four-number array, as a colour when every number lies in
/// `[0, 1]`; returns whether it changed.
pub fn render_numeric_quad(
    ui: &mut dyn UiBuilder,
    label: &str,
    arr: &mut [Value],
    _theme: &UiTheme,
) -> bool {
    // Heuristic: 4 numbers all in `[0, 1]` are likely a colour.
    let all_unit = arr.iter().all(|v| {
        let n = v.as_f64().unwrap_or(f64::NAN);
        (0.0..=1.0).contains(&n)
    });
    if all_unit {
        let mut rgba = [
            json_to_f32(Some(&arr[0])),
            json_to_f32(Some(&arr[1])),
            json_to_f32(Some(&arr[2])),
            json_to_f32(Some(&arr[3])),
        ];
        let changed = ui.color_edit(label, &mut rgba);
        if changed {
            for (i, v) in rgba.iter().enumerate() {
                arr[i] = f32_to_json(*v);
            }
        }
        return changed;
    }
    // Otherwise — generic 4-component drag row.
    let mut changed = false;
    ui.horizontal(&mut |row| {
        row.label(label);
        for v in arr.iter_mut() {
            let mut local = json_to_f32(Some(&*v));
            if row.drag_value_f32("", &mut local, 0.05) {
                *v = f32_to_json(local);
                changed = true;
            }
        }
    });
    changed
}

// ─── numeric bridge ─────────────────────────────────────────────────

/// Returns the float a JSON value holds: a number, or the `{"$float": …}`
/// marker the engine writes for an infinity or a NaN; `0` for anything else.
pub fn json_to_f32(v: Option<&Value>) -> f32 {
    match v {
        Some(v) => v
            .as_f64()
            .map(|f| f as f32)
            .or_else(|| float_marker(v))
            .unwrap_or(0.0),
        None => 0.0,
    }
}

/// Returns `f` as JSON: a number, or the `{"$float": …}` marker for an
/// infinity or a NaN, never the `null` the component would refuse.
pub fn f32_to_json(f: f32) -> Value {
    let marker = |text: &str| serde_json::json!({ FLOAT_KEY: text });
    if f.is_nan() {
        marker("nan")
    } else if f == f32::INFINITY {
        marker("inf")
    } else if f == f32::NEG_INFINITY {
        marker("-inf")
    } else {
        serde_json::Number::from_f64(f as f64)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    }
}

/// The key of the marker the engine writes for a float JSON has no number
/// for (`khora_data::scene::inspector_json`).
const FLOAT_KEY: &str = "$float";

/// Returns whether `map` is the `{"$float": …}` marker of a float.
pub fn is_float_marker(map: &serde_json::Map<String, Value>) -> bool {
    map.len() == 1 && map.get(FLOAT_KEY).is_some_and(Value::is_string)
}

/// The float of a `{"$float": "inf" | "-inf" | "nan"}` marker.
fn float_marker(v: &Value) -> Option<f32> {
    match v.as_object()?.get(FLOAT_KEY)?.as_str()? {
        "inf" => Some(f32::INFINITY),
        "-inf" => Some(f32::NEG_INFINITY),
        "nan" => Some(f32::NAN),
        _ => None,
    }
}

/// Returns a `snake_case` field name as the label `"Snake Case"`.
pub fn humanise(field: &str) -> String {
    let mut out = String::with_capacity(field.len());
    let mut up = true;
    for c in field.chars() {
        if c == '_' {
            out.push(' ');
            up = true;
        } else if up {
            out.extend(c.to_uppercase());
            up = false;
        } else {
            out.push(c);
        }
    }
    out
}
