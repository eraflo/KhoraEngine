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

//! Properties Inspector — fully generic, JSON-driven component cards.
//!
//! There is **no per-component code path**. Every component on the
//! inspected entity is captured as `serde_json::Value` by the macro-
//! generated `ComponentRegistration::to_json`, walked by [`render_value`],
//! and committed back through `PropertyEdit::SetComponentJson` →
//! `ComponentRegistration::from_json` (same round-trip the scene
//! serializer uses for save/load).
//!
//! The walker dispatches by JSON shape:
//!
//! | Shape                                 | Widget |
//! |---------------------------------------|--------|
//! | `Object { x, y, z }`                  | Vec3 with red/green/blue X/Y/Z badges |
//! | `Object { x, y, z, w }`               | Quaternion (4 drag values) |
//! | `Object { r, g, b, a }`               | colour swatch + RGBA dragvalues |
//! | `Object { "Variant": <inner> }`       | enum: variant title + recurse on inner |
//! | `Object { … }` (other)                | indented row of children |
//! | `Number`                              | DragValue |
//! | `Bool`                                | checkbox |
//! | `String`                              | text input |
//! | `Array<f64>` (3 or 4)                 | falls through to Vec3 / Vec4 / colour |
//! | `Null`                                | dim "null" label |
//!
//! Adding a new ECS component costs **zero** editor code as long as it
//! derives `Component` (which auto-registers `to_json` + `from_json`
//! through the macro).

use std::sync::{Arc, Mutex};

use khora_sdk::editor_ui::*;
use khora_sdk::prelude::ecs::*;

use crate::widgets::chrome::{paint_panel_header, panel_tab};
use crate::widgets::enum_variants::editable_variants;
use crate::widgets::inspector::paint_inspector_header;
use crate::widgets::paint::{paint_icon, with_alpha};

const HEADER_HEIGHT: f32 = 34.0;
const INSPECTOR_HEADER_HEIGHT: f32 = 64.0;
const SUBTAB_HEIGHT: f32 = 28.0;
const CARD_HEADER_H: f32 = 30.0;

/// Components that are inherent to a vessel (auto-managed by the engine
/// or surfaced elsewhere) — never offered in the "+ Add Component" menu
/// nor rendered as a card. `Name` lives in the inspector header instead.
const INHERENT_COMPONENTS: &[&str] = &[
    "GlobalTransform",
    "Parent",
    "Children",
    "HandleComponent<GpuMesh>",
    "Name",
];

pub struct PropertiesPanel {
    state: Arc<Mutex<EditorState>>,
    command_history: Arc<Mutex<CommandHistory>>,
    theme: EditorTheme,
    sub_tab: SubTab,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubTab {
    Properties,
    Debug,
}

impl PropertiesPanel {
    pub fn new(
        state: Arc<Mutex<EditorState>>,
        history: Arc<Mutex<CommandHistory>>,
        theme: EditorTheme,
    ) -> Self {
        Self {
            state,
            command_history: history,
            theme,
            sub_tab: SubTab::Properties,
        }
    }
}

impl EditorPanel for PropertiesPanel {
    fn id(&self) -> &str {
        "properties"
    }
    fn title(&self) -> &str {
        "Inspector"
    }

    fn preferred_size(&self) -> Option<f32> {
        Some(320.0)
    }

    fn ui(&mut self, ui: &mut dyn UiBuilder) {
        let theme = self.theme.clone();
        let panel_rect = ui.panel_rect();
        let [px, py, pw, _] = panel_rect;

        // ── Panel header strip ────────────────────────
        paint_panel_header(ui, panel_rect, HEADER_HEIGHT, &theme);
        let tab_y = py + (HEADER_HEIGHT - 22.0) * 0.5;

        let action_icons: &[(Icon, &str)] =
            &[(Icon::More, "p-act-more"), (Icon::Lock, "p-act-lock")];
        let icons_total_w = action_icons.len() as f32 * 22.0 + 8.0;
        let icons_left = px + pw - icons_total_w;
        let _ = icons_left;

        let _ = panel_tab(
            ui,
            "p-tab-inspector",
            [px + 6.0, tab_y],
            "Inspector",
            None,
            true,
            &theme,
        );

        let mut ax = px + pw - 12.0;
        for (icon, salt) in action_icons {
            ax -= 22.0;
            let int = ui.interact_rect(salt, [ax, py + 6.0, 22.0, 22.0]);
            if int.hovered {
                ui.paint_rect_filled([ax, py + 6.0], [22.0, 22.0], theme.surface_active, 4.0);
            }
            paint_icon(ui, [ax + 5.0, py + 11.0], *icon, 13.0, theme.text_dim);
        }

        // ── Snapshot ─────────────────────────────────
        let entity_data = {
            let s = match self.state.lock() {
                Ok(s) => s,
                Err(_) => return,
            };
            s.single_selected().and_then(|e| {
                s.inspected
                    .clone()
                    .filter(|i| i.entity == e)
                    .map(|i| (e, i))
            })
        };

        // ── Inspector entity header ──────────────────
        let header_origin = [px, py + HEADER_HEIGHT];
        let after_header = match &entity_data {
            Some((entity, inspected)) => {
                let icon = pick_icon(inspected);
                let id_label = format!("id 0x{:04X}", entity.index);
                let type_tag = pick_type_tag(inspected);
                paint_inspector_header(
                    ui,
                    header_origin,
                    pw,
                    icon,
                    &inspected.name,
                    type_tag,
                    "Active",
                    theme.success,
                    Some(&id_label),
                    &theme,
                )
            }
            None => {
                ui.paint_rect_filled(
                    header_origin,
                    [pw, INSPECTOR_HEADER_HEIGHT],
                    theme.surface,
                    0.0,
                );
                ui.paint_text_styled(
                    [px + pw * 0.5, py + HEADER_HEIGHT + 22.0],
                    "Select an entity",
                    13.0,
                    theme.text_muted,
                    FontFamilyHint::Proportional,
                    TextAlign::Center,
                );
                py + HEADER_HEIGHT + INSPECTOR_HEADER_HEIGHT
            }
        };

        // ── Sub-tabs ─────────────────────────────────
        let subtab_y = after_header + 8.0;
        let subtab_w = pw - 12.0;
        ui.paint_rect_filled(
            [px + 6.0, subtab_y],
            [subtab_w, SUBTAB_HEIGHT],
            theme.background,
            theme.radius_md,
        );
        ui.paint_rect_stroke(
            [px + 6.0, subtab_y],
            [subtab_w, SUBTAB_HEIGHT],
            with_alpha(theme.separator, 0.55),
            theme.radius_md,
            1.0,
        );
        let segment_w = (subtab_w - 4.0) / 2.0;
        let segments = [(SubTab::Properties, "Properties"), (SubTab::Debug, "Debug")];
        for (i, (tab, label)) in segments.iter().enumerate() {
            let sx = px + 8.0 + i as f32 * segment_w;
            let active = self.sub_tab == *tab;
            let interaction = ui.interact_rect(
                &format!("p-sub-{}", label),
                [sx, subtab_y + 2.0, segment_w, SUBTAB_HEIGHT - 4.0],
            );
            if active {
                ui.paint_rect_filled(
                    [sx, subtab_y + 2.0],
                    [segment_w, SUBTAB_HEIGHT - 4.0],
                    theme.surface_active,
                    theme.radius_md - 2.0,
                );
            }
            ui.paint_text_styled(
                [sx + segment_w * 0.5, subtab_y + 7.0],
                label,
                11.0,
                if active { theme.text } else { theme.text_dim },
                FontFamilyHint::Proportional,
                TextAlign::Center,
            );
            if interaction.clicked {
                self.sub_tab = *tab;
            }
        }

        // ── Body ────────────────────────────────────
        let body_y = subtab_y + SUBTAB_HEIGHT + 10.0;
        let body_h = (panel_rect[1] + panel_rect[3] - body_y - 6.0).max(0.0);
        let body_rect = [px + 6.0, body_y, pw - 12.0, body_h];

        let (entity, inspected) = match entity_data {
            Some(e) => e,
            None => return,
        };

        let theme_for_closure = theme.clone();

        match self.sub_tab {
            SubTab::Properties => {
                ui.region_at(body_rect, &mut |ui_inner| {
                    let mut state_guard = match self.state.lock() {
                        Ok(s) => s,
                        Err(_) => return,
                    };
                    let panel_rect_inner = ui_inner.panel_rect();
                    let card_x = panel_rect_inner[0];
                    let card_w = panel_rect_inner[2];

                    let mut edits: Vec<PropertyEdit> = Vec::new();

                    for cj in inspected.components_json.iter() {
                        if INHERENT_COMPONENTS.contains(&cj.type_name.as_str()) {
                            continue;
                        }
                        let title = cj.type_name.clone();
                        let mut value = cj.value.clone();
                        let icon = icon_for_domain_tag(cj.domain);
                        let mut changed = false;
                        render_card(
                            ui_inner,
                            entity,
                            &title,
                            icon,
                            None,
                            true, // removable — INHERENT_COMPONENTS already filtered above
                            card_x,
                            card_w,
                            &theme_for_closure,
                            &mut state_guard,
                            &mut |ui_b| {
                                changed = render_value(ui_b, &mut value, &theme_for_closure);
                            },
                        );
                        if changed {
                            edits.push(PropertyEdit::SetComponentJson {
                                entity,
                                type_name: cj.type_name.clone(),
                                value,
                            });
                        }
                    }

                    ui_inner.spacing(8.0);
                    render_add_component(ui_inner, entity, &inspected, &mut state_guard);

                    for e in edits.drain(..) {
                        state_guard.push_edit(e);
                    }
                });
            }
            SubTab::Debug => {
                let history = self.command_history.clone();
                let undo_desc = history
                    .lock()
                    .ok()
                    .and_then(|h| h.undo_description().map(|s| s.to_owned()))
                    .unwrap_or_else(|| "(none)".to_owned());
                let redo_desc = history
                    .lock()
                    .ok()
                    .and_then(|h| h.redo_description().map(|s| s.to_owned()))
                    .unwrap_or_else(|| "(none)".to_owned());
                ui.region_at(body_rect, &mut |ui_inner| {
                    ui_inner.colored_label(theme_for_closure.text_dim, "Command history:");
                    ui_inner.colored_label(
                        theme_for_closure.text_muted,
                        &format!("Undo: {} | Redo: {}", undo_desc, redo_desc),
                    );
                });
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────
//  Card frame (chevron + icon + title + body)
// ─────────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn render_card(
    ui: &mut dyn UiBuilder,
    entity: EntityId,
    title: &str,
    icon: Icon,
    enabled: Option<bool>,
    removable: bool,
    card_x: f32,
    card_w: f32,
    theme: &EditorTheme,
    state: &mut EditorState,
    body: &mut dyn FnMut(&mut dyn UiBuilder),
) {
    let card_id = format!("{}::{}", entity.index, title);
    let open = *state
        .inspector_card_open
        .entry(card_id.clone())
        .or_insert(true);

    let cursor_y = ui.cursor_pos()[1];
    let header_rect = [card_x, cursor_y, card_w, CARD_HEADER_H];

    ui.paint_rect_filled(
        [card_x, cursor_y],
        [card_w, CARD_HEADER_H],
        theme.surface_elevated,
        theme.radius_md,
    );
    ui.paint_rect_stroke(
        [card_x, cursor_y],
        [card_w, CARD_HEADER_H],
        with_alpha(theme.separator, 0.55),
        theme.radius_md,
        1.0,
    );

    let chev = if open {
        Icon::ChevronDown
    } else {
        Icon::ChevronRight
    };
    paint_icon(
        ui,
        [card_x + 8.0, cursor_y + 9.0],
        chev,
        12.0,
        theme.text_muted,
    );
    paint_icon(
        ui,
        [card_x + 26.0, cursor_y + 8.0],
        icon,
        14.0,
        theme.primary_dim,
    );
    crate::widgets::paint::paint_text_size(
        ui,
        [card_x + 46.0, cursor_y + 9.0],
        title,
        12.0,
        theme.text,
    );

    // Trailing edge: optional toggle, then optional trash button.
    let mut right_edge = card_x + card_w - 10.0;

    if let Some(en) = enabled {
        let toggle_w = 26.0;
        let toggle_h = 14.0;
        let tx = right_edge - toggle_w;
        let ty = cursor_y + (CARD_HEADER_H - toggle_h) * 0.5;
        let track_color = if en {
            theme.primary
        } else {
            theme.surface_active
        };
        ui.paint_rect_filled([tx, ty], [toggle_w, toggle_h], track_color, 999.0);
        let knob_x = if en { tx + 13.0 } else { tx + 1.0 };
        ui.paint_circle_filled([knob_x + 6.0, ty + toggle_h * 0.5], 5.5, theme.text);
        right_edge = tx - 6.0;
    }

    if removable {
        let trash_w = 22.0;
        let trash_h = 22.0;
        let tx = right_edge - trash_w;
        let ty = cursor_y + (CARD_HEADER_H - trash_h) * 0.5;
        let trash_int =
            ui.interact_rect(&format!("card-rm-{}", card_id), [tx, ty, trash_w, trash_h]);
        let trash_color = if trash_int.hovered {
            theme.error
        } else {
            theme.text_muted
        };
        if trash_int.hovered {
            ui.paint_rect_filled(
                [tx, ty],
                [trash_w, trash_h],
                with_alpha(theme.error, 0.12),
                4.0,
            );
        }
        paint_icon(ui, [tx + 5.0, ty + 5.0], Icon::Trash, 12.0, trash_color);
        if trash_int.clicked {
            state.pending_edits.push(PropertyEdit::RemoveComponent {
                entity,
                type_name: title.to_string(),
            });
        }
    }

    let header_int = ui.interact_rect(&format!("card-hdr-{}", card_id), header_rect);
    if header_int.clicked {
        state.inspector_card_open.insert(card_id, !open);
    }

    ui.spacing(CARD_HEADER_H + 4.0);

    if open {
        ui.indent("card-body", &mut |ui_b| {
            body(ui_b);
        });
        ui.spacing(10.0);
    } else {
        ui.spacing(4.0);
    }
}

// ─────────────────────────────────────────────────────────────────────
//  Generic JSON walker — single source of truth for "show a component"
// ─────────────────────────────────────────────────────────────────────

/// Renders a top-level component value. Returns true if the user mutated
/// any leaf — the caller queues a `SetComponentJson` edit in that case.
fn render_value(
    ui: &mut dyn UiBuilder,
    value: &mut serde_json::Value,
    theme: &EditorTheme,
) -> bool {
    use serde_json::Value;
    match value {
        Value::Object(map) => render_object(ui, map, theme),
        Value::Array(arr) => render_array(ui, "", arr, theme),
        // A bare scalar component (unit struct, newtype) — wrap in a single
        // unnamed row so the generic code path handles it identically.
        _ => render_scalar_inline(ui, "(value)", value, theme),
    }
}

/// Walks a JSON object, picking the right widget for each shape.
fn render_object(
    ui: &mut dyn UiBuilder,
    map: &mut serde_json::Map<String, serde_json::Value>,
    theme: &EditorTheme,
) -> bool {
    use serde_json::Value;

    // 1. Single-key objects are serde-tagged enum variants.
    if map.len() == 1 {
        let key = map.keys().next().cloned().unwrap();

        // If this enum is registered as editor-switchable, render a combo
        // box and replace the payload with the new variant's defaults on
        // selection.
        if let Some(variants) = editable_variants(&key) {
            let names: Vec<&str> = variants.iter().map(|(n, _)| *n).collect();
            let mut current = variants
                .iter()
                .position(|(n, _)| *n == key.as_str())
                .unwrap_or(0);
            let combo_changed = ui.combo_box("Variant", &mut current, &names);
            if combo_changed {
                if let Some((_, default_full)) = variants.get(current) {
                    if let Value::Object(new_map) = default_full {
                        map.clear();
                        for (k, v) in new_map.iter() {
                            map.insert(k.clone(), v.clone());
                        }
                        return true;
                    }
                }
            }
            // Fall through to recurse into the (possibly newly-typed) inner.
            let inner_key = map.keys().next().cloned().unwrap();
            let inner = map.get_mut(&inner_key).unwrap();
            return match inner {
                Value::Object(m) => render_object(ui, m, theme),
                Value::Null => false,
                _ => render_scalar_inline(ui, "value", inner, theme),
            };
        }

        // Unknown enum — read-only label fallback.
        let inner = map.get_mut(&key).unwrap();
        ui.colored_label(theme.text_dim, &format!("Variant: {}", key));
        return match inner {
            Value::Object(m) => render_object(ui, m, theme),
            Value::Null => false,
            _ => render_scalar_inline(ui, "value", inner, theme),
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
    value: &mut serde_json::Value,
    theme: &EditorTheme,
) -> bool {
    use serde_json::Value;
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
    arr: &mut [serde_json::Value],
    theme: &EditorTheme,
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

fn render_scalar_inline(
    ui: &mut dyn UiBuilder,
    label: &str,
    value: &mut serde_json::Value,
    theme: &EditorTheme,
) -> bool {
    render_field(ui, label, value, theme)
}

// ─── shape detection ────────────────────────────────────────────────

fn is_vec3(map: &serde_json::Map<String, serde_json::Value>) -> bool {
    map.len() == 3
        && map.get("x").is_some_and(|v| v.is_number())
        && map.get("y").is_some_and(|v| v.is_number())
        && map.get("z").is_some_and(|v| v.is_number())
}

fn is_quat(map: &serde_json::Map<String, serde_json::Value>) -> bool {
    map.len() == 4
        && map.get("x").is_some_and(|v| v.is_number())
        && map.get("y").is_some_and(|v| v.is_number())
        && map.get("z").is_some_and(|v| v.is_number())
        && map.get("w").is_some_and(|v| v.is_number())
}

fn is_color(map: &serde_json::Map<String, serde_json::Value>) -> bool {
    map.len() == 4
        && map.get("r").is_some_and(|v| v.is_number())
        && map.get("g").is_some_and(|v| v.is_number())
        && map.get("b").is_some_and(|v| v.is_number())
        && map.get("a").is_some_and(|v| v.is_number())
}

// ─── leaf widgets ───────────────────────────────────────────────────

fn render_vec3(
    ui: &mut dyn UiBuilder,
    map: &mut serde_json::Map<String, serde_json::Value>,
    _theme: &EditorTheme,
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

fn render_quat(
    ui: &mut dyn UiBuilder,
    map: &mut serde_json::Map<String, serde_json::Value>,
) -> bool {
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

fn render_color(
    ui: &mut dyn UiBuilder,
    map: &mut serde_json::Map<String, serde_json::Value>,
    _theme: &EditorTheme,
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

fn render_number_row(ui: &mut dyn UiBuilder, label: &str, value: &mut serde_json::Value) -> bool {
    let mut local = json_to_f32(Some(&*value));
    let changed = ui.drag_value_f32(label, &mut local, 0.05);
    if changed {
        *value = f32_to_json(local);
    }
    changed
}

fn render_numeric_triple(
    ui: &mut dyn UiBuilder,
    label: &str,
    arr: &mut [serde_json::Value],
) -> bool {
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

fn render_numeric_quad(
    ui: &mut dyn UiBuilder,
    label: &str,
    arr: &mut [serde_json::Value],
    _theme: &EditorTheme,
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

fn json_to_f32(v: Option<&serde_json::Value>) -> f32 {
    v.and_then(|v| v.as_f64()).map(|f| f as f32).unwrap_or(0.0)
}

fn f32_to_json(f: f32) -> serde_json::Value {
    serde_json::Number::from_f64(f as f64)
        .map(serde_json::Value::Number)
        .unwrap_or(serde_json::Value::Null)
}

fn humanise(field: &str) -> String {
    // snake_case → "Snake Case" — keeps the inspector readable without
    // the engine having to ship a separate display-name table.
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

// ─────────────────────────────────────────────────────────────────────
//  Inspector header decorations
// ─────────────────────────────────────────────────────────────────────

fn pick_icon(i: &InspectedEntity) -> Icon {
    // Icon hint based on which "interesting" component the entity has.
    // Generic enough to not bias toward any specific subset.
    let names: std::collections::HashSet<&str> = i
        .components_json
        .iter()
        .map(|c| c.type_name.as_str())
        .collect();
    if names.contains("Camera") {
        Icon::Camera
    } else if names.contains("Light") {
        Icon::Light
    } else if names.contains("AudioSource") || names.contains("AudioListener") {
        Icon::Music
    } else {
        Icon::Cube
    }
}

fn pick_type_tag(i: &InspectedEntity) -> &'static str {
    let names: std::collections::HashSet<&str> = i
        .components_json
        .iter()
        .map(|c| c.type_name.as_str())
        .collect();
    if names.contains("Camera") {
        "Camera"
    } else if names.contains("Light") {
        "Light"
    } else if names.contains("AudioSource") || names.contains("AudioListener") {
        "Audio"
    } else {
        "Mesh"
    }
}

fn icon_for_domain_tag(tag: Option<u8>) -> Icon {
    match tag {
        Some(0) => Icon::Axes,   // Spatial
        Some(1) => Icon::Image,  // Render
        Some(2) => Icon::Music,  // Audio
        Some(3) => Icon::Zap,    // Physics
        Some(4) => Icon::Layers, // UI
        _ => Icon::More,
    }
}

// ─────────────────────────────────────────────────────────────────────
//  Add Component menu — bucketed by SemanticDomain (live World)
// ─────────────────────────────────────────────────────────────────────

fn category_label_for_tag(tag: Option<u8>) -> &'static str {
    match tag {
        Some(0) => "Spatial",
        Some(1) => "Render",
        Some(2) => "Audio",
        Some(3) => "Physics",
        Some(4) => "UI",
        _ => "Other",
    }
}

fn render_add_component(
    ui: &mut dyn UiBuilder,
    entity: EntityId,
    inspected: &InspectedEntity,
    state: &mut EditorState,
) {
    let mut buckets: std::collections::BTreeMap<u8, Vec<String>> =
        std::collections::BTreeMap::new();
    let mut other: Vec<String> = Vec::new();

    let already_present: std::collections::HashSet<&str> = inspected
        .components_json
        .iter()
        .map(|c| c.type_name.as_str())
        .collect();

    for reg in inventory::iter::<khora_sdk::ComponentRegistration> {
        if INHERENT_COMPONENTS.contains(&reg.type_name) {
            continue;
        }
        if already_present.contains(reg.type_name) {
            continue;
        }
        let domain_tag = state.component_domain_registry.get(reg.type_name).copied();

        if let Some(tag) = domain_tag {
            buckets
                .entry(tag)
                .or_default()
                .push(reg.type_name.to_string());
        } else {
            other.push(reg.type_name.to_string());
        }
    }
    let pending: std::cell::Cell<Option<String>> = std::cell::Cell::new(None);
    ui.menu_button("+ Add Component", &mut |ui_m| {
        for (tag, items) in &buckets {
            if items.is_empty() {
                continue;
            }
            let label = category_label_for_tag(Some(*tag)).to_string();
            ui_m.menu_button(&label, &mut |ui_s| {
                for n in items {
                    if ui_s.button(n) {
                        pending.set(Some(n.clone()));
                        ui_s.close_menu();
                    }
                }
            });
        }
        if !other.is_empty() {
            ui_m.menu_button("Other", &mut |ui_s| {
                for n in &other {
                    if ui_s.button(n) {
                        pending.set(Some(n.clone()));
                        ui_s.close_menu();
                    }
                }
            });
        }
    });
    if let Some(name) = pending.into_inner() {
        state.pending_add_component = Some((entity, name));
    }
}
