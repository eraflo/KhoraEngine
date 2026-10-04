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

use khora_sdk::editor_ui::{Icon, UiBuilder, UiTheme};
use khora_tool_ui::widgets::paint::{icon_centered, tint};
use khora_tool_ui::widgets::{self};
use serde_json::Value;

use crate::widgets::enum_variants::editable_variants;

use super::renderers::{
    humanise, is_color, is_quat, is_vec3, render_color, render_number_row, render_numeric_quad,
    render_numeric_triple, render_quat, render_vec3,
};

/// What a field row asks of its component, beyond its own value.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldAction {
    /// Put the prefab's value back at this path.
    Revert(Vec<String>),
    /// Write this path's value into the prefab.
    Apply(Vec<String>),
}

/// The fields of a component that override its prefab, and what the rows
/// that show them were asked to do.
///
/// A row is marked when the field it shows is overridden, or holds one that
/// is: a vector row whose `x` differs, a struct one of whose fields does.
#[derive(Default)]
pub struct Marks<'a> {
    overridden: &'a [Vec<String>],
    path: Vec<String>,
    /// What the marked rows were asked this frame.
    pub actions: Vec<FieldAction>,
}

impl<'a> Marks<'a> {
    /// Marks for a component whose overridden fields are `overridden`.
    pub fn new(overridden: &'a [Vec<String>]) -> Self {
        Self {
            overridden,
            path: Vec::new(),
            actions: Vec::new(),
        }
    }

    fn covers(&self) -> bool {
        !self.path.is_empty() && self.overridden.iter().any(|p| p.starts_with(&self.path))
    }

    /// A dot in the row's gutter and a revert at its end — the dot and the
    /// glyph together, so colour is never the only sign; right-click offers
    /// both actions in words.
    fn mark_row(&mut self, ui: &mut dyn UiBuilder, top: [f32; 2], theme: &UiTheme) {
        let path = self.path.clone();
        ui.paint_circle_filled([top[0] - 7.0, top[1] + 9.0], 2.5, theme.primary);
        let panel = ui.panel_rect();
        let size = 18.0;
        let rect = [panel[0] + panel[2] - size - 2.0, top[1], size, size];
        let hit = ui.interact_rect(&format!("revert::{}", path.join(".")), rect);
        if hit.hovered {
            widgets::fill(ui, rect, tint(theme.primary, 0.14), theme.radius_sm);
        }
        icon_centered(
            ui,
            rect,
            Icon::Revert,
            11.0,
            if hit.hovered {
                theme.text
            } else {
                theme.text_muted
            },
        );
        if hit.clicked {
            self.actions.push(FieldAction::Revert(path.clone()));
        }
        let actions = &mut self.actions;
        ui.context_menu_last(&mut |menu| {
            if menu.button("Revert to prefab") {
                actions.push(FieldAction::Revert(path.clone()));
                menu.close_menu();
            }
            if menu.button("Apply to prefab") {
                actions.push(FieldAction::Apply(path.clone()));
                menu.close_menu();
            }
        });
    }
}

/// Renders a top-level component value, marking the rows of the fields that
/// override the component's prefab and gathering what those rows were asked
/// in `marks`. Returns true if the user mutated any leaf — the caller queues
/// a `SetComponentJson` edit in that case.
pub fn render_value_marked(
    ui: &mut dyn UiBuilder,
    value: &mut Value,
    theme: &UiTheme,
    marks: &mut Marks<'_>,
) -> bool {
    match value {
        Value::Object(map) => render_object(ui, map, theme, marks),
        Value::Array(arr) => render_array(ui, "", arr, theme, marks),
        // A bare scalar component (unit struct, newtype) — wrap in a
        // single unnamed row so the generic code path handles it
        // identically.
        _ => render_field(ui, "(value)", value, theme, marks),
    }
}

/// Walks a JSON object, picking the right widget for each shape.
fn render_object(
    ui: &mut dyn UiBuilder,
    map: &mut serde_json::Map<String, Value>,
    theme: &UiTheme,
    marks: &mut Marks<'_>,
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
            // Salted by the variant set, which identifies the enum *type* and
            // stays put when the selection changes — unlike the current
            // variant name, and unlike the shared "Variant" label.
            let combo_salt = format!("variant::{}", names.join("|"));
            let combo_changed = ui.combo_box(&combo_salt, "Variant", &mut current, &names);
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
            marks.path.push(inner_key);
            let changed = match inner {
                Value::Object(m) => render_object(ui, m, theme, marks),
                Value::Null => false,
                _ => render_field(ui, "value", inner, theme, marks),
            };
            marks.path.pop();
            return changed;
        }

        // Unknown enum — read-only label fallback.
        let inner = map.get_mut(&key).unwrap();
        ui.colored_label(theme.text_dim, &format!("Variant: {}", key));
        marks.path.push(key);
        let changed = match inner {
            Value::Object(m) => render_object(ui, m, theme, marks),
            Value::Null => false,
            _ => render_field(ui, "value", inner, theme, marks),
        };
        marks.path.pop();
        return changed;
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
        marks.path.push(key);
        if render_field(ui, &label, val, theme, marks) {
            changed = true;
        }
        marks.path.pop();
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
    marks: &mut Marks<'_>,
) -> bool {
    let marked = marks.covers();
    let top = ui.cursor_pos();
    let changed = render_field_value(ui, label, value, theme, marks);
    if marked {
        marks.mark_row(ui, top, theme);
    }
    changed
}

fn render_field_value(
    ui: &mut dyn UiBuilder,
    label: &str,
    value: &mut Value,
    theme: &UiTheme,
    marks: &mut Marks<'_>,
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
        Value::Array(arr) => render_array(ui, label, arr, theme, marks),
        Value::Object(_) => {
            // Nested struct / enum — collapsing block keeps the layout
            // readable for deeply nested components.
            let mut changed = false;
            ui.collapsing(label, true, &mut |inner| {
                if let Value::Object(m) = value {
                    if render_object(inner, m, theme, marks) {
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
    _marks: &mut Marks<'_>,
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
        // A list differs whole: its elements carry no mark of their own.
        let mut unmarked = Marks::default();
        for (i, v) in arr.iter_mut().enumerate() {
            if render_field(inner, &format!("[{}]", i), v, theme, &mut unmarked) {
                changed = true;
            }
        }
    });
    changed
}
