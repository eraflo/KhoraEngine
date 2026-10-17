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

//! One field of a script-declared component, drawn by its Ergon type.
//!
//! | `ErgonType`        | Widget |
//! |--------------------|--------|
//! | `bool`             | checkbox |
//! | `int` / `float`    | drag value (an `int` stays whole) |
//! | `Duration`         | drag value in seconds, `s` after it |
//! | `Angle`            | drag value in degrees, `deg` after it — stored in radians |
//! | ranged number      | the drag value kept inside `[Range(…)]`, a gauge under it |
//! | `string`           | text field |
//! | `Vec2/3/4`, `Quat` | one drag value per axis |
//! | `Color`            | colour swatch |
//! | `Entity`           | a slot naming the entity; drop one from the hierarchy |
//! | `T?`               | `T`, or `none`, with a toggle between them |
//! | `T[]`              | a row per item, an add button, a context menu to move or remove |
//! | struct             | a collapsing block, a row per field |
//!
//! A read-only field — what the engine writes — is the same row as text.
//! A `[Tooltip(…)]` is said by hovering a number's label, or the control of
//! any other field.

use std::collections::HashMap;

use khora_sdk::editor_ui::{FontFamilyHint, UiBuilder, UiTheme};
use khora_sdk::khora_core::math::{LinearRgba, Quaternion, Vec2, Vec3, Vec4};
use khora_sdk::khora_core::script::{ErgonType, FieldRange, ScriptValue};
use khora_sdk::prelude::ecs::EntityId;
use khora_tool_ui::widgets::paint::tint;
use khora_tool_ui::widgets::{self};

use crate::drag_payload::{payload_is_entity, unpack_entity};

use super::super::renderers::humanise;
use super::hints::range_note;
use super::ranged::{render_label, render_number, render_read_only_gauge};

/// What a field row needs besides its value: the theme, the names of the
/// scene's entities, and a stable id for the widgets it allocates.
pub struct FieldContext<'a> {
    /// The inspector theme.
    pub theme: &'a UiTheme,
    /// Every entity of the scene tree, by id — what an entity slot shows.
    pub names: &'a HashMap<EntityId, String>,
    /// The id the row's own widgets are salted with.
    pub id: String,
    /// The field's `[Range(…)]`, held by each number it holds.
    pub range: Option<FieldRange>,
    /// The field's `[Tooltip(…)]`, said by its own row only.
    pub tooltip: Option<&'a str>,
}

impl FieldContext<'_> {
    /// A field of a struct the row holds: its hints are not the row's.
    fn child(&self, part: &str) -> FieldContext<'_> {
        FieldContext {
            theme: self.theme,
            names: self.names,
            id: format!("{}.{part}", self.id),
            range: None,
            tooltip: None,
        }
    }

    /// An item of a list the row holds: held to the row's range.
    fn item(&self, part: &str) -> FieldContext<'_> {
        FieldContext {
            range: self.range,
            ..self.child(part)
        }
    }
}

/// The value a field of `ty` starts with when the author gives it one: the
/// type's neutral value — what an `Optional` set from `none` or an array's
/// new item holds.
pub fn neutral(ty: &ErgonType) -> ScriptValue {
    match ty {
        ErgonType::Bool => ScriptValue::Bool(false),
        ErgonType::Int => ScriptValue::Int(0),
        ErgonType::Float | ErgonType::Duration | ErgonType::Angle => ScriptValue::Float(0.0),
        ErgonType::Str => ScriptValue::Str(String::new()),
        ErgonType::Engine("Vec2") => ScriptValue::Vec2(Vec2::ZERO),
        ErgonType::Engine("Vec3") => ScriptValue::Vec3(Vec3::ZERO),
        ErgonType::Engine("Vec4") => ScriptValue::Vec4(Vec4::ZERO),
        ErgonType::Engine("Quat") => ScriptValue::Quat(Quaternion::IDENTITY),
        ErgonType::Engine("Color") => ScriptValue::Color(LinearRgba::WHITE),
        ErgonType::Array(_) => ScriptValue::Array(Vec::new()),
        ErgonType::Struct {
            fields: Some(fields),
            ..
        } => ScriptValue::Struct(
            fields
                .iter()
                .map(|(name, ty)| (name.clone(), neutral(ty)))
                .collect(),
        ),
        ErgonType::Struct { fields: None, .. } => ScriptValue::Struct(Vec::new()),
        ErgonType::Entity | ErgonType::Optional(_) | ErgonType::Engine(_) | ErgonType::Any => {
            ScriptValue::Null
        }
    }
}

/// `value` as one line of text, read the way its type reads: a duration in
/// seconds, an angle in degrees, an entity by its name.
pub fn value_text(
    value: &ScriptValue,
    ty: &ErgonType,
    names: &HashMap<EntityId, String>,
) -> String {
    match (value, ty) {
        (ScriptValue::Null, _) => "none".to_owned(),
        (ScriptValue::Float(seconds), ErgonType::Duration) => format!("{} s", trim(*seconds)),
        (ScriptValue::Float(radians), ErgonType::Angle) => {
            format!("{} deg", trim(radians.to_degrees()))
        }
        (_, ErgonType::Optional(inner)) => value_text(value, inner, names),
        (ScriptValue::Unit, _) => "()".to_owned(),
        (ScriptValue::Bool(b), _) => b.to_string(),
        (ScriptValue::Int(i), _) => i.to_string(),
        (ScriptValue::Float(f), _) => trim(*f),
        (ScriptValue::Str(s), _) => format!("\"{s}\""),
        (ScriptValue::Vec2(v), _) => format!("({}, {})", trim(v.x), trim(v.y)),
        (ScriptValue::Vec3(v), _) => format!("({}, {}, {})", trim(v.x), trim(v.y), trim(v.z)),
        (ScriptValue::Vec4(v), _) => format!(
            "({}, {}, {}, {})",
            trim(v.x),
            trim(v.y),
            trim(v.z),
            trim(v.w)
        ),
        (ScriptValue::Quat(q), _) => format!(
            "({}, {}, {}, {})",
            trim(q.x),
            trim(q.y),
            trim(q.z),
            trim(q.w)
        ),
        (ScriptValue::Color(c), _) => format!(
            "rgba({}, {}, {}, {})",
            trim(c.r),
            trim(c.g),
            trim(c.b),
            trim(c.a)
        ),
        (ScriptValue::Entity(entity), _) => entity_text(*entity, names),
        (ScriptValue::Array(items), _) => format!("{} items", items.len()),
        (ScriptValue::Struct(fields), _) => format!("{} fields", fields.len()),
    }
}

/// An entity as a slot shows it: its name, or its index where the scene tree
/// does not know it.
pub fn entity_text(entity: EntityId, names: &HashMap<EntityId, String>) -> String {
    names
        .get(&entity)
        .cloned()
        .unwrap_or_else(|| format!("Entity {}", entity.index))
}

/// A float as an author reads it: no trailing zeros, at most four decimals.
fn trim(value: f32) -> String {
    let text = format!("{value:.4}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" {
        "0".to_owned()
    } else {
        text.to_owned()
    }
}

/// Draws the field `name` of type `ty` holding `value`; returns whether the
/// author changed it. `read_only` draws it as text.
pub fn render_field(
    ui: &mut dyn UiBuilder,
    name: &str,
    ty: &ErgonType,
    value: &mut ScriptValue,
    read_only: bool,
    cx: &FieldContext<'_>,
) -> bool {
    let label = humanise(name);
    let changed = if read_only {
        render_read_only(ui, &label, ty, value, cx);
        false
    } else {
        render_editable(ui, &label, ty, value, cx)
    };
    if let Some(tooltip) = cx.tooltip.filter(|_| !labels_itself(ty, value, read_only)) {
        ui.tooltip_for_last(tooltip);
    }
    changed
}

/// Whether the row of a `ty` field holding `value` draws its label with the
/// field's tooltip: a number's, a read-only scalar's, an empty optional's.
fn labels_itself(ty: &ErgonType, value: &ScriptValue, read_only: bool) -> bool {
    match (ty, value) {
        (_, ScriptValue::Array(items)) if !items.is_empty() => false,
        (_, ScriptValue::Struct(fields)) if !fields.is_empty() => false,
        _ if read_only => true,
        (ErgonType::Optional(_), ScriptValue::Null) => true,
        (ErgonType::Optional(inner), _) => labels_itself(inner, value, read_only),
        (ErgonType::Int, ScriptValue::Int(_)) => true,
        (ErgonType::Float | ErgonType::Duration | ErgonType::Angle, ScriptValue::Float(_)) => true,
        _ => false,
    }
}

fn render_read_only(
    ui: &mut dyn UiBuilder,
    label: &str,
    ty: &ErgonType,
    value: &ScriptValue,
    cx: &FieldContext<'_>,
) {
    // Each row, and whether it is an item — held to the field's range — or
    // a struct's field, which is not.
    let nested = match value {
        ScriptValue::Array(items) if !items.is_empty() => Some(
            items
                .iter()
                .enumerate()
                .map(|(i, item)| (format!("[{i}]"), item_type(ty), item.clone(), true))
                .collect::<Vec<_>>(),
        ),
        ScriptValue::Struct(fields) if !fields.is_empty() => Some(
            fields
                .iter()
                .map(|(name, item)| (humanise(name), field_type(ty, name), item.clone(), false))
                .collect(),
        ),
        _ => None,
    };
    match nested {
        Some(rows) => ui.collapsing(label, false, &mut |inner| {
            for (i, (row_label, row_ty, row_value, item)) in rows.iter().enumerate() {
                let part = i.to_string();
                let row_cx = if *item {
                    cx.item(&part)
                } else {
                    cx.child(&part)
                };
                render_read_only(inner, row_label, row_ty, row_value, &row_cx);
            }
        }),
        None => {
            let text = value_text(value, ty, cx.names);
            let leaf = item_or_self(ty);
            let note = cx.range.and_then(|range| range_note(&leaf, value, range));
            ui.horizontal(&mut |row| {
                render_label(row, cx, label, cx.theme.text_dim);
                match &note {
                    Some(note) => row.colored_label_with_tooltip(cx.theme.warning, &text, note),
                    None => row.colored_label(cx.theme.text_muted, &text),
                }
            });
            render_read_only_gauge(ui, cx, &leaf, value);
        }
    }
}

/// `ty` without the optional around it: what a value it holds is.
fn item_or_self(ty: &ErgonType) -> ErgonType {
    match ty {
        ErgonType::Optional(inner) => item_or_self(inner),
        other => other.clone(),
    }
}

/// The type of an item of a `ty` array — `Any` where `ty` says nothing.
fn item_type(ty: &ErgonType) -> ErgonType {
    match ty {
        ErgonType::Array(inner) => (**inner).clone(),
        ErgonType::Optional(inner) => item_type(inner),
        _ => ErgonType::Any,
    }
}

/// The type of the field `name` of a `ty` struct — `Any` where unknown.
fn field_type(ty: &ErgonType, name: &str) -> ErgonType {
    match ty {
        ErgonType::Struct {
            fields: Some(fields),
            ..
        } => fields
            .iter()
            .find(|(known, _)| known == name)
            .map_or(ErgonType::Any, |(_, ty)| ty.clone()),
        ErgonType::Optional(inner) => field_type(inner, name),
        _ => ErgonType::Any,
    }
}

/// The type `value` shows it holds, where the declaration says `Any`.
fn type_of(value: &ScriptValue) -> ErgonType {
    match value {
        ScriptValue::Bool(_) => ErgonType::Bool,
        ScriptValue::Int(_) => ErgonType::Int,
        ScriptValue::Float(_) => ErgonType::Float,
        ScriptValue::Str(_) => ErgonType::Str,
        ScriptValue::Vec2(_) => ErgonType::Engine("Vec2"),
        ScriptValue::Vec3(_) => ErgonType::Engine("Vec3"),
        ScriptValue::Vec4(_) => ErgonType::Engine("Vec4"),
        ScriptValue::Quat(_) => ErgonType::Engine("Quat"),
        ScriptValue::Color(_) => ErgonType::Engine("Color"),
        ScriptValue::Entity(_) => ErgonType::Entity,
        _ => ErgonType::Any,
    }
}

fn render_editable(
    ui: &mut dyn UiBuilder,
    label: &str,
    ty: &ErgonType,
    value: &mut ScriptValue,
    cx: &FieldContext<'_>,
) -> bool {
    match ty {
        ErgonType::Any => {
            let seen = type_of(value);
            if seen == ErgonType::Any {
                render_read_only(ui, label, ty, value, cx);
                false
            } else {
                render_editable(ui, label, &seen, value, cx)
            }
        }
        ErgonType::Optional(inner) => render_optional(ui, label, inner, value, cx),
        ErgonType::Array(inner) => render_array(ui, label, inner, value, cx),
        ErgonType::Struct { .. } => render_struct(ui, label, ty, value, cx),
        ErgonType::Entity => render_entity(ui, label, value, false, cx),
        _ => render_leaf(ui, label, ty, value, cx),
    }
}

/// A scalar, text or engine value.
fn render_leaf(
    ui: &mut dyn UiBuilder,
    label: &str,
    ty: &ErgonType,
    value: &mut ScriptValue,
    cx: &FieldContext<'_>,
) -> bool {
    if let Some(changed) = render_number(ui, label, ty, value, cx) {
        return changed;
    }
    match (ty, &mut *value) {
        (ErgonType::Bool, ScriptValue::Bool(b)) => ui.checkbox(b, label),
        (ErgonType::Str, ScriptValue::Str(text)) => {
            let mut changed = false;
            ui.horizontal(&mut |row| {
                row.label(label);
                changed = row.text_edit_singleline(text);
            });
            changed
        }
        (ErgonType::Engine(_), ScriptValue::Vec3(v)) => {
            let mut axes = [v.x, v.y, v.z];
            let changed = ui.vec3_editor(label, &mut axes, 0.05);
            if changed {
                *v = Vec3::new(axes[0], axes[1], axes[2]);
            }
            changed
        }
        (ErgonType::Engine(_), ScriptValue::Vec2(v)) => {
            let mut axes = [v.x, v.y];
            let changed = render_axes(ui, label, &mut axes);
            if changed {
                *v = Vec2::new(axes[0], axes[1]);
            }
            changed
        }
        (ErgonType::Engine(_), ScriptValue::Vec4(v)) => {
            let mut axes = [v.x, v.y, v.z, v.w];
            let changed = render_axes(ui, label, &mut axes);
            if changed {
                *v = Vec4::new(axes[0], axes[1], axes[2], axes[3]);
            }
            changed
        }
        (ErgonType::Engine(_), ScriptValue::Quat(q)) => {
            let mut axes = [q.x, q.y, q.z, q.w];
            let changed = render_axes(ui, label, &mut axes);
            if changed {
                *q = Quaternion::new(axes[0], axes[1], axes[2], axes[3]).normalize();
            }
            changed
        }
        (ErgonType::Engine(_), ScriptValue::Color(c)) => {
            let mut rgba = [c.r, c.g, c.b, c.a];
            let changed = ui.color_edit(label, &mut rgba);
            if changed {
                *c = LinearRgba::new(rgba[0], rgba[1], rgba[2], rgba[3]);
            }
            changed
        }
        // A value its type does not describe — an `int` the row holds as a
        // float, say: shown, not edited, until the row holds its type again.
        _ => {
            render_read_only(ui, label, ty, value, cx);
            false
        }
    }
}

/// One drag value per axis, on one row.
fn render_axes(ui: &mut dyn UiBuilder, label: &str, axes: &mut [f32]) -> bool {
    let mut changed = false;
    ui.horizontal(&mut |row| {
        row.label(label);
        for axis in axes.iter_mut() {
            changed |= row.drag_value_f32("", axis, 0.05);
        }
    });
    changed
}

/// `T?`: the value, or `none`, and the toggle between them.
fn render_optional(
    ui: &mut dyn UiBuilder,
    label: &str,
    inner: &ErgonType,
    value: &mut ScriptValue,
    cx: &FieldContext<'_>,
) -> bool {
    if *inner == ErgonType::Entity {
        return render_entity(ui, label, value, true, cx);
    }
    if *value == ScriptValue::Null {
        let mut set = false;
        ui.horizontal(&mut |row| {
            render_label(row, cx, label, cx.theme.text);
            row.colored_label(cx.theme.text_muted, "none");
            set = row.small_button("Set");
        });
        if set {
            *value = neutral(inner);
        }
        return set;
    }
    let mut changed = render_editable(ui, label, inner, value, cx);
    let mut cleared = false;
    ui.context_menu_last(&mut |menu| {
        if menu.button("Set to none") {
            cleared = true;
            menu.close_menu();
        }
    });
    if cleared {
        *value = ScriptValue::Null;
        changed = true;
    }
    changed
}

/// `T[]`: a row per item — each with a context menu to move or remove it —
/// and a button that adds one.
fn render_array(
    ui: &mut dyn UiBuilder,
    label: &str,
    inner: &ErgonType,
    value: &mut ScriptValue,
    cx: &FieldContext<'_>,
) -> bool {
    let ScriptValue::Array(items) = value else {
        render_read_only(
            ui,
            label,
            &ErgonType::Array(Box::new(inner.clone())),
            value,
            cx,
        );
        return false;
    };
    let mut changed = false;
    let mut moved: Option<(usize, usize)> = None;
    let mut removed: Option<usize> = None;
    let header = format!("{label}  ({})", items.len());
    ui.collapsing(&header, true, &mut |block| {
        let count = items.len();
        for (i, item) in items.iter_mut().enumerate() {
            changed |= render_editable(
                block,
                &format!("[{i}]"),
                inner,
                item,
                &cx.item(&i.to_string()),
            );
            block.context_menu_last(&mut |menu| {
                if i > 0 && menu.button("Move up") {
                    moved = Some((i, i - 1));
                    menu.close_menu();
                }
                if i + 1 < count && menu.button("Move down") {
                    moved = Some((i, i + 1));
                    menu.close_menu();
                }
                if menu.button("Remove") {
                    removed = Some(i);
                    menu.close_menu();
                }
            });
        }
        if block.small_button("+ Add item") {
            items.push(neutral(inner));
            changed = true;
        }
    });
    if let Some((from, to)) = moved {
        items.swap(from, to);
        changed = true;
    }
    if let Some(i) = removed {
        items.remove(i);
        changed = true;
    }
    changed
}

/// A struct: a collapsing block, a row per field.
fn render_struct(
    ui: &mut dyn UiBuilder,
    label: &str,
    ty: &ErgonType,
    value: &mut ScriptValue,
    cx: &FieldContext<'_>,
) -> bool {
    let ScriptValue::Struct(fields) = value else {
        render_read_only(ui, label, ty, value, cx);
        return false;
    };
    let mut changed = false;
    ui.collapsing(label, true, &mut |block| {
        for (name, field) in fields.iter_mut() {
            let field_ty = field_type(ty, name);
            changed |= render_editable(block, &humanise(name), &field_ty, field, &cx.child(name));
        }
    });
    changed
}

/// An entity slot: the entity's name in a well, where an entity dragged from
/// the hierarchy is dropped. An optional one is cleared with its context
/// menu.
fn render_entity(
    ui: &mut dyn UiBuilder,
    label: &str,
    value: &mut ScriptValue,
    optional: bool,
    cx: &FieldContext<'_>,
) -> bool {
    const SLOT_H: f32 = 22.0;
    let theme = cx.theme;
    let held = match value {
        ScriptValue::Entity(entity) => Some(*entity),
        _ => None,
    };
    let mut dropped = None;
    let mut cleared = false;
    ui.horizontal(&mut |row| {
        row.label(label);
        let width = row.available_width().max(80.0);
        let rect = row.allocate_size([width, SLOT_H]);
        let hit = row.interact_rect(&format!("{}.entity-slot", cx.id), rect);
        let dragging = row.is_drag_active();
        let fill = if dragging && hit.hovered {
            tint(theme.primary, 0.18)
        } else {
            theme.surface_interactive
        };
        widgets::fill(row, rect, fill, theme.radius_sm);
        if dragging {
            row.paint_rect_stroke(
                [rect[0], rect[1]],
                [rect[2], rect[3]],
                theme.primary,
                theme.radius_sm,
                1.0,
            );
        }
        let (text, color) = match held {
            Some(entity) => (entity_text(entity, cx.names), theme.text),
            None => ("none — drop an entity".to_owned(), theme.text_muted),
        };
        let size = theme.font_size_body;
        row.paint_text_styled(
            [rect[0] + 8.0, rect[1] + (SLOT_H - size) * 0.5 - 1.0],
            &text,
            size,
            color,
            FontFamilyHint::Proportional,
            khora_sdk::editor_ui::TextAlign::Left,
        );
        if let Some(payload) = row.dnd_take_drop_payload() {
            if payload_is_entity(payload) {
                dropped = Some(unpack_entity(payload));
            }
        }
        if optional && held.is_some() {
            row.context_menu_last(&mut |menu| {
                if menu.button("Set to none") {
                    cleared = true;
                    menu.close_menu();
                }
            });
        }
    });
    if let Some(entity) = dropped {
        *value = ScriptValue::Entity(entity);
        return true;
    }
    if cleared {
        *value = ScriptValue::Null;
        return true;
    }
    false
}
