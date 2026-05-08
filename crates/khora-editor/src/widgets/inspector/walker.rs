// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Generic JSON walker — single source of truth for "show a component".
//!
//! Dispatch by JSON shape:
//!
//! | Shape                                 | Widget |
//! |---------------------------------------|--------|
//! | `Object { x, y, z }`                  | Vec3 with X/Y/Z badges |
//! | `Object { x, y, z, w }`               | Quaternion (4 drag values) |
//! | `Object { r, g, b, a }`               | colour swatch + RGBA dragvalues |
//! | `Object { "Variant": <inner> }`       | enum: variant title + recurse |
//! | `Object { … }` (other)                | indented row of children |
//! | `Number`                              | DragValue |
//! | `Bool`                                | checkbox |
//! | `String`                              | text input |
//! | `Array<f64>` (3 or 4)                 | falls through to Vec3 / colour |
//! | `Null`                                | dim "null" label |

use khora_sdk::editor_ui::{UiTheme, UiBuilder};
use serde_json::Value;

use crate::widgets::enum_variants::editable_variants;

use super::renderers::{
    humanise, is_color, is_quat, is_vec3, render_color, render_number_row, render_numeric_quad,
    render_numeric_triple, render_quat, render_vec3,
};

/// Renders a top-level component value. Returns true if the user mutated
/// any leaf — the caller queues a `SetComponentJson` edit in that case.
pub fn render_value(
    ui: &mut dyn UiBuilder,
    value: &mut Value,
    theme: &UiTheme,
) -> bool {
    match value {
        Value::Object(map) => render_object(ui, map, theme),
        Value::Array(arr) => render_array(ui, "", arr, theme),
        // A bare scalar component (unit struct, newtype) — wrap in a
        // single unnamed row so the generic code path handles it
        // identically.
        _ => render_field(ui, "(value)", value, theme),
    }
}

/// Walks a JSON object, picking the right widget for each shape.
fn render_object(
    ui: &mut dyn UiBuilder,
    map: &mut serde_json::Map<String, Value>,
    theme: &UiTheme,
) -> bool {
    // 1. Single-key objects are serde-tagged enum variants.
    if map.len() == 1 {
        let key = map.keys().next().cloned().unwrap();

        // If this enum is registered as editor-switchable, render a
        // combo-box and replace the payload with the new variant's
        // defaults on selection.
        if let Some(variants) = editable_variants(&key) {
            let names: Vec<&str> = variants.iter().map(|(n, _)| *n).collect();
            let mut current = variants
                .iter()
                .position(|(n, _)| *n == key.as_str())
                .unwrap_or(0);
            let combo_changed = ui.combo_box("Variant", &mut current, &names);
            if combo_changed {
                if let Some((_, Value::Object(new_map))) = variants.get(current) {
                    map.clear();
                    for (k, v) in new_map.iter() {
                        map.insert(k.clone(), v.clone());
                    }
                    return true;
                }
            }
            // Fall through to recurse into the (possibly newly-typed)
            // inner.
            let inner_key = map.keys().next().cloned().unwrap();
            let inner = map.get_mut(&inner_key).unwrap();
            return match inner {
                Value::Object(m) => render_object(ui, m, theme),
                Value::Null => false,
                _ => render_field(ui, "value", inner, theme),
            };
        }

        // Unknown enum — read-only label fallback.
        let inner = map.get_mut(&key).unwrap();
        ui.colored_label(theme.text_dim, &format!("Variant: {}", key));
        return match inner {
            Value::Object(m) => render_object(ui, m, theme),
            Value::Null => false,
            _ => render_field(ui, "value", inner, theme),
        };
    }

    // 2. Specialised shapes detected by field-name signature.
    if is_color(map) {
        return render_color(ui, map, theme);
    }
    if is_vec3(map) {
        return render_vec3(ui, map, theme);
    }
    if is_quat(map) {
        return render_quat(ui, map);
    }

    // 3. Generic object — one row per field.
    let mut changed = false;
    let keys: Vec<String> = map.keys().cloned().collect();
    for key in keys {
        let label = humanise(&key);
        let val = map.get_mut(&key).unwrap();
        if render_field(ui, &label, val, theme) {
            changed = true;
        }
    }
    changed
}

/// Renders a single labelled row. Picks a widget by leaf type or
/// recurses for nested structure.
fn render_field(
    ui: &mut dyn UiBuilder,
    label: &str,
    value: &mut Value,
    theme: &UiTheme,
) -> bool {
    match value {
        Value::Bool(b) => {
            let mut local = *b;
            let changed = ui.checkbox(&mut local, label);
            if changed {
                *value = Value::Bool(local);
            }
            changed
        }
        Value::Number(_) => render_number_row(ui, label, value),
        Value::String(s) => {
            let mut local = s.clone();
            let mut changed = false;
            ui.horizontal(&mut |row| {
                row.label(label);
                if row.text_edit_singleline(&mut local) {
                    changed = true;
                }
            });
            if changed {
                *value = Value::String(local);
            }
            changed
        }
        Value::Null => {
            ui.colored_label(theme.text_muted, &format!("{}: null", label));
            false
        }
        Value::Array(arr) => render_array(ui, label, arr, theme),
        Value::Object(_) => {
            // Nested struct / enum — collapsing block keeps the layout
            // readable for deeply nested components.
            let mut changed = false;
            ui.collapsing(label, true, &mut |inner| {
                if let Value::Object(m) = value {
                    if render_object(inner, m, theme) {
                        changed = true;
                    }
                }
            });
            changed
        }
    }
}

/// Generic array row. Numeric arrays of length 3 / 4 are treated like
/// Vec3 / Vec4 with axis colours; everything else falls back to "[i] = …"
/// rows.
fn render_array(
    ui: &mut dyn UiBuilder,
    label: &str,
    arr: &mut [Value],
    theme: &UiTheme,
) -> bool {
    if arr.iter().all(|v| v.is_number()) {
        match arr.len() {
            3 => return render_numeric_triple(ui, label, arr),
            4 => return render_numeric_quad(ui, label, arr, theme),
            _ => {}
        }
    }

    let mut changed = false;
    ui.collapsing(label, true, &mut |inner| {
        for (i, v) in arr.iter_mut().enumerate() {
            if render_field(inner, &format!("[{}]", i), v, theme) {
                changed = true;
            }
        }
    });
    changed
}
