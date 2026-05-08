// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Per-shape leaf renderers + numeric helpers.
//!
//! Renderers are pure functions matched by JSON shape. The walker (in
//! `widgets::inspector::walker`) iterates the shape detectors (`is_*`) in
//! order and dispatches to the matching renderer. Adding a new shape =
//! adding a `match_shape` predicate and a `render_*` function and wiring
//! them in `walker::render_object`.

use khora_sdk::editor_ui::{UiTheme, UiBuilder};
use serde_json::Value;

// ─── shape detection ────────────────────────────────────────────────

pub fn is_vec3(map: &serde_json::Map<String, Value>) -> bool {
    map.len() == 3
        && map.get("x").is_some_and(|v| v.is_number())
        && map.get("y").is_some_and(|v| v.is_number())
        && map.get("z").is_some_and(|v| v.is_number())
}

pub fn is_quat(map: &serde_json::Map<String, Value>) -> bool {
    map.len() == 4
        && map.get("x").is_some_and(|v| v.is_number())
        && map.get("y").is_some_and(|v| v.is_number())
        && map.get("z").is_some_and(|v| v.is_number())
        && map.get("w").is_some_and(|v| v.is_number())
}

pub fn is_color(map: &serde_json::Map<String, Value>) -> bool {
    map.len() == 4
        && map.get("r").is_some_and(|v| v.is_number())
        && map.get("g").is_some_and(|v| v.is_number())
        && map.get("b").is_some_and(|v| v.is_number())
        && map.get("a").is_some_and(|v| v.is_number())
}

// ─── leaf widgets ───────────────────────────────────────────────────

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

pub fn render_number_row(ui: &mut dyn UiBuilder, label: &str, value: &mut Value) -> bool {
    let mut local = json_to_f32(Some(&*value));
    let changed = ui.drag_value_f32(label, &mut local, 0.05);
    if changed {
        *value = f32_to_json(local);
    }
    changed
}

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

pub fn json_to_f32(v: Option<&Value>) -> f32 {
    v.and_then(|v| v.as_f64()).map(|f| f as f32).unwrap_or(0.0)
}

pub fn f32_to_json(f: f32) -> Value {
    serde_json::Number::from_f64(f as f64)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

/// `snake_case` → `"Snake Case"` — keeps the inspector readable without
/// the engine having to ship a separate display-name table.
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
