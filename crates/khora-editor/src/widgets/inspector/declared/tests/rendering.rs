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

//! What a declared field's row draws for its hints: where its tooltip lands,
//! and which numbers its range holds — through a builder that keeps, as the
//! egui one does, a "last widget" a tooltip attaches to: a widget sets it, a
//! label does not, a row hands its last widget to its parent, a collapsing
//! block keeps its own.
//!
//! The builder also plays what the pointer brings — a drag over the region
//! drawn, a drop on it, a menu opened, a button clicked — for the slots that
//! answer them.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use khora_sdk::editor_ui::{FontFamilyHint, Interaction, TextAlign, UiBuilder, UiTheme};
use khora_sdk::khora_core::script::{ErgonType, FieldRange, ScriptValue};
use khora_sdk::ViewportTextureHandle;

use super::super::fields::{render_field, FieldContext};

/// What the builder was asked to draw, in order.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Drawn {
    /// A widget, numbered in drawing order.
    Widget(usize),
    /// A tooltip attached to the last widget, if any.
    Tooltip { on: Option<usize>, text: String },
    /// A label, in `color`, saying `tooltip` when hovered.
    Label {
        text: String,
        color: [f32; 4],
        tooltip: Option<String>,
    },
    /// A filled rect, in `color`.
    Fill([f32; 4]),
    /// A stroked rect, in `color`, `thickness` wide.
    Stroke { color: [f32; 4], thickness: f32 },
    /// Painted text, in `color`, in the font `family`.
    Text {
        text: String,
        color: [f32; 4],
        family: FontFamilyHint,
    },
    /// A button — or a selectable label, a menu's button — saying `text`.
    Button(String),
    /// A single-line text field.
    TextField,
    /// A scroll area opened; what it holds follows.
    Scroll,
}

/// What the pointer brings this frame, shared by every builder nested in
/// the frame's.
#[derive(Default)]
pub(super) struct Input {
    /// A drag is in flight.
    pub dragging: bool,
    /// The payload of a drag hovering the region drawn.
    pub hover: Option<u64>,
    /// The payload released on the region drawn — the first to take it has it.
    pub drop: Option<u64>,
    /// The pointer is on every region drawn: it hovers it, and is inside it.
    pub pointer_on: bool,
    /// A menu a primary click opens (`menu_on_click_last`) is open.
    pub click_menu_open: bool,
    /// A context menu is open.
    pub context_menu_open: bool,
    /// What the author clicks: every button whose text holds one of these.
    pub clicks: Vec<String>,
}

#[derive(Default)]
pub(super) struct Spy {
    drawn: Rc<RefCell<Vec<Drawn>>>,
    next: Rc<RefCell<usize>>,
    input: Rc<RefCell<Input>>,
    last: Option<usize>,
    cursor: [f32; 2],
}

impl Spy {
    /// A builder the pointer brings `input` to.
    pub(super) fn with(input: Input) -> Spy {
        Spy {
            input: Rc::new(RefCell::new(input)),
            ..Spy::default()
        }
    }

    fn nested(&self) -> Spy {
        Spy {
            drawn: Rc::clone(&self.drawn),
            next: Rc::clone(&self.next),
            input: Rc::clone(&self.input),
            last: None,
            cursor: self.cursor,
        }
    }

    /// Draws a button saying `text`; whether the author clicks it.
    fn button_drawn(&mut self, text: &str) -> bool {
        self.widget();
        self.drawn.borrow_mut().push(Drawn::Button(text.to_owned()));
        self.input
            .borrow()
            .clicks
            .iter()
            .any(|click| text.contains(click.as_str()))
    }

    /// Everything drawn, in order.
    pub(super) fn drawn(&self) -> Vec<Drawn> {
        self.drawn.borrow().clone()
    }

    /// The text of every button drawn, in order.
    pub(super) fn buttons(&self) -> Vec<String> {
        self.drawn
            .borrow()
            .iter()
            .filter_map(|drawn| match drawn {
                Drawn::Button(text) => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// Every text drawn to be read — a label's, a painted one's, not an
    /// icon's — and its colour.
    pub(super) fn texts(&self) -> Vec<(String, [f32; 4])> {
        self.drawn
            .borrow()
            .iter()
            .filter_map(|drawn| match drawn {
                Drawn::Label { text, color, .. } => Some((text.clone(), *color)),
                Drawn::Text {
                    text,
                    color,
                    family,
                } if *family != FontFamilyHint::Icons => Some((text.clone(), *color)),
                _ => None,
            })
            .collect()
    }

    /// Every icon glyph painted, and its colour.
    pub(super) fn glyphs(&self) -> Vec<(String, [f32; 4])> {
        self.drawn
            .borrow()
            .iter()
            .filter_map(|drawn| match drawn {
                Drawn::Text {
                    text,
                    color,
                    family: FontFamilyHint::Icons,
                } => Some((text.clone(), *color)),
                _ => None,
            })
            .collect()
    }

    /// How many rects were stroked.
    pub(super) fn strokes(&self) -> usize {
        self.drawn
            .borrow()
            .iter()
            .filter(|drawn| matches!(drawn, Drawn::Stroke { .. }))
            .count()
    }

    /// The colour of every rect filled.
    pub(super) fn fills(&self) -> Vec<[f32; 4]> {
        self.drawn
            .borrow()
            .iter()
            .filter_map(|drawn| match drawn {
                Drawn::Fill(color) => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// Every tooltip said — attached to a widget, or held by a label.
    pub(super) fn said_tooltips(&self) -> Vec<String> {
        self.drawn
            .borrow()
            .iter()
            .filter_map(|drawn| match drawn {
                Drawn::Tooltip { text, .. } => Some(text.clone()),
                Drawn::Label {
                    tooltip: Some(text),
                    ..
                } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// Whether a single-line text field was drawn.
    pub(super) fn has_text_field(&self) -> bool {
        self.drawn.borrow().contains(&Drawn::TextField)
    }

    /// The drop no one took.
    pub(super) fn drop_left(&self) -> Option<u64> {
        self.input.borrow().drop
    }

    fn widget(&mut self) {
        let id = {
            let mut next = self.next.borrow_mut();
            *next += 1;
            *next
        };
        self.drawn.borrow_mut().push(Drawn::Widget(id));
        self.last = Some(id);
    }

    fn label_drawn(&mut self, text: &str, color: [f32; 4], tooltip: Option<&str>) {
        self.drawn.borrow_mut().push(Drawn::Label {
            text: text.to_owned(),
            color,
            tooltip: tooltip.map(str::to_owned),
        });
    }

    fn widgets(&self) -> Vec<usize> {
        self.drawn
            .borrow()
            .iter()
            .filter_map(|drawn| match drawn {
                Drawn::Widget(id) => Some(*id),
                _ => None,
            })
            .collect()
    }

    fn tooltips(&self) -> Vec<(Option<usize>, String)> {
        self.drawn
            .borrow()
            .iter()
            .filter_map(|drawn| match drawn {
                Drawn::Tooltip { on, text } => Some((*on, text.clone())),
                _ => None,
            })
            .collect()
    }

    fn used(&self, color: [f32; 4]) -> bool {
        self.drawn.borrow().iter().any(|drawn| match drawn {
            Drawn::Fill(fill) => *fill == color,
            Drawn::Label { color: text, .. } => *text == color,
            _ => false,
        })
    }
}

impl UiBuilder for Spy {
    fn paint_rect_filled(&mut self, _min: [f32; 2], _size: [f32; 2], color: [f32; 4], _r: f32) {
        self.drawn.borrow_mut().push(Drawn::Fill(color));
    }
    fn paint_rect_stroke(&mut self, _: [f32; 2], _: [f32; 2], color: [f32; 4], _: f32, w: f32) {
        self.drawn.borrow_mut().push(Drawn::Stroke {
            color,
            thickness: w,
        });
    }
    fn paint_line(&mut self, _: [f32; 2], _: [f32; 2], _: [f32; 4], _: f32) {}
    fn paint_circle_filled(&mut self, _: [f32; 2], _: f32, _: [f32; 4]) {}
    fn paint_circle_stroke(&mut self, _: [f32; 2], _: f32, _: [f32; 4], _: f32) {}
    fn paint_text(&mut self, _: [f32; 2], color: [f32; 4], text: &str) {
        self.drawn.borrow_mut().push(Drawn::Text {
            text: text.to_owned(),
            color,
            family: FontFamilyHint::Proportional,
        });
    }
    fn paint_text_styled(
        &mut self,
        _: [f32; 2],
        text: &str,
        _: f32,
        color: [f32; 4],
        family: FontFamilyHint,
        _: TextAlign,
    ) {
        self.drawn.borrow_mut().push(Drawn::Text {
            text: text.to_owned(),
            color,
            family,
        });
    }
    fn paint_path_filled(&mut self, _: &[[f32; 2]], _: [f32; 4]) {}
    fn interact_rect(&mut self, _id_salt: &str, _rect: [f32; 4]) -> Interaction {
        self.widget();
        let on = self.input.borrow().pointer_on;
        Interaction {
            hovered: on,
            contains_pointer: on,
            ..Interaction::default()
        }
    }
    fn is_drag_active(&self) -> bool {
        self.input.borrow().dragging
    }
    fn dnd_hover_payload(&self) -> Option<u64> {
        self.input.borrow().hover
    }
    fn dnd_take_drop_payload(&mut self) -> Option<u64> {
        self.input.borrow_mut().drop.take()
    }
    fn menu_on_click_last(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let open = self.input.borrow().click_menu_open;
        if open {
            let mut menu = self.nested();
            f(&mut menu);
        }
    }
    fn menu_button(&mut self, text: &str, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        if self.button_drawn(text) {
            let mut menu = self.nested();
            f(&mut menu);
        }
    }
    fn region_at(&mut self, _: &str, _: [f32; 4], f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let mut nested = self.nested();
        f(&mut nested);
    }
    fn cursor_pos(&self) -> [f32; 2] {
        self.cursor
    }
    fn allocate_size(&mut self, size: [f32; 2]) -> [f32; 4] {
        let rect = [self.cursor[0], self.cursor[1], size[0], size[1]];
        self.cursor[1] += size[1];
        rect
    }
    fn panel_rect(&self) -> [f32; 4] {
        [0.0, 0.0, 300.0, 600.0]
    }
    fn screen_rect(&self) -> [f32; 4] {
        [0.0, 0.0, 300.0, 600.0]
    }
    fn available_width(&self) -> f32 {
        300.0
    }
    fn available_height(&self) -> f32 {
        600.0
    }
    fn heading(&mut self, text: &str) {
        self.label_drawn(text, [1.0; 4], None);
    }
    fn label(&mut self, text: &str) {
        self.label_drawn(text, [1.0; 4], None);
    }
    fn colored_label(&mut self, color: [f32; 4], text: &str) {
        self.label_drawn(text, color, None);
    }
    fn colored_label_with_tooltip(&mut self, color: [f32; 4], text: &str, tooltip: &str) {
        self.label_drawn(text, color, Some(tooltip));
    }
    fn small_label(&mut self, text: &str) {
        self.label_drawn(text, [1.0; 4], None);
    }
    fn monospace(&mut self, text: &str) {
        self.label_drawn(text, [1.0; 4], None);
    }
    fn button(&mut self, text: &str) -> bool {
        self.button_drawn(text)
    }
    fn small_button(&mut self, text: &str) -> bool {
        self.button_drawn(text)
    }
    fn selectable_label(&mut self, _: bool, text: &str) -> bool {
        self.button_drawn(text)
    }
    fn selectable_label_double_clicked(&mut self, _: bool, text: &str) -> bool {
        self.button_drawn(text)
    }
    fn checkbox(&mut self, _: &mut bool, _: &str) -> bool {
        self.widget();
        false
    }
    fn drag_value_f32(&mut self, _: &str, _: &mut f32, _: f32) -> bool {
        self.widget();
        false
    }
    fn slider_f32(&mut self, _: &str, _: &mut f32, _: f32, _: f32) -> bool {
        self.widget();
        false
    }
    fn text_edit_singleline(&mut self, _: &mut String) -> bool {
        self.widget();
        self.drawn.borrow_mut().push(Drawn::TextField);
        false
    }
    fn vec3_editor(&mut self, _: &str, _: &mut [f32; 3], _: f32) -> bool {
        self.widget();
        false
    }
    fn color_edit(&mut self, _: &str, _: &mut [f32; 4]) -> bool {
        self.widget();
        false
    }
    fn combo_box(&mut self, _: &str, _: &str, _: &mut usize, _: &[&str]) -> bool {
        self.widget();
        false
    }
    fn horizontal(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let mut nested = self.nested();
        f(&mut nested);
        self.cursor = nested.cursor;
        if nested.last.is_some() {
            self.last = nested.last;
        }
    }
    fn vertical(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let mut nested = self.nested();
        f(&mut nested);
        self.cursor = nested.cursor;
    }
    fn collapsing(&mut self, _: &str, _: bool, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        // The header is the block's widget, as egui's builder records it.
        self.widget();
        let mut nested = self.nested();
        f(&mut nested);
        self.cursor = nested.cursor;
    }
    fn indent(&mut self, _: &str, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let mut nested = self.nested();
        f(&mut nested);
        self.cursor = nested.cursor;
    }
    fn scroll_area(&mut self, _: &str, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        self.drawn.borrow_mut().push(Drawn::Scroll);
        let mut nested = self.nested();
        f(&mut nested);
    }
    fn scroll_delta_in(&self, _: [f32; 4]) -> f32 {
        0.0
    }
    fn pointer_position(&self) -> Option<[f32; 2]> {
        None
    }
    fn separator(&mut self) {}
    fn spacing(&mut self, points: f32) {
        self.cursor[1] += points;
    }
    fn is_last_item_double_clicked(&self) -> bool {
        false
    }
    fn is_last_item_hovered(&self) -> bool {
        false
    }
    fn is_last_item_enter_pressed(&self) -> bool {
        false
    }
    fn is_last_item_escape_pressed(&self) -> bool {
        false
    }
    fn context_menu_last(&mut self, f: &mut dyn FnMut(&mut dyn UiBuilder)) {
        let open = self.input.borrow().context_menu_open;
        if open {
            let mut menu = self.nested();
            f(&mut menu);
        }
    }
    fn tooltip_for_last(&mut self, text: &str) {
        self.drawn.borrow_mut().push(Drawn::Tooltip {
            on: self.last,
            text: text.to_owned(),
        });
    }
    fn viewport_image(&mut self, _: ViewportTextureHandle, _: [f32; 2]) -> Option<[f32; 2]> {
        None
    }
}

fn context<'a>(
    theme: &'a UiTheme,
    names: &'a HashMap<khora_sdk::prelude::ecs::EntityId, String>,
    range: Option<FieldRange>,
    tooltip: Option<&'a str>,
) -> FieldContext<'a> {
    FieldContext {
        theme,
        names,
        id: "declared.1.Stats.field".to_owned(),
        range,
        tooltip,
        assets: &[],
        asset_epoch: 0,
        memory: None,
    }
}

fn int_array(items: &[i64]) -> (ErgonType, ScriptValue) {
    (
        ErgonType::Array(Box::new(ErgonType::Int)),
        ScriptValue::Array(items.iter().map(|i| ScriptValue::Int(*i)).collect()),
    )
}

/// A list field's tooltip is the list's: it never lands on the control of the
/// field drawn before it.
#[test]
fn a_list_fields_tooltip_never_lands_on_the_field_before_it() {
    let theme = UiTheme::default();
    let names = HashMap::new();
    for read_only in [false, true] {
        let mut ui = Spy::default();
        // `float power;` then `[Tooltip("How many levels")] int[] levels;`.
        let mut power = ScriptValue::Float(2.0);
        render_field(
            &mut ui,
            "power",
            &ErgonType::Float,
            &mut power,
            read_only,
            &context(&theme, &names, None, None),
        );
        let before = ui.widgets();
        let (ty, mut levels) = int_array(&[1, 2]);
        render_field(
            &mut ui,
            "levels",
            &ty,
            &mut levels,
            read_only,
            &context(&theme, &names, None, Some("How many levels")),
        );
        for (on, text) in ui.tooltips() {
            assert!(
                on.is_none_or(|widget| !before.contains(&widget)),
                "read_only {read_only}: `{text}` lands on widget {on:?}, drawn for `power` ({before:?})"
            );
        }
    }
}

/// A read-only list of ranged numbers — a system's, a state's — holds each
/// item to the range, as the editable one does: an item outside it is amber.
#[test]
fn a_read_only_list_holds_each_item_to_its_fields_range() {
    let theme = UiTheme::default();
    let names = HashMap::new();
    let bounds = Some(FieldRange {
        min: 0.0,
        max: 100.0,
    });
    let (ty, levels) = int_array(&[50, 150]);

    let mut editable = Spy::default();
    let mut value = levels.clone();
    render_field(
        &mut editable,
        "levels",
        &ty,
        &mut value,
        false,
        &context(&theme, &names, bounds, None),
    );
    assert!(
        editable.used(theme.warning),
        "the editable list shows 150 outside 0–100"
    );

    let mut read_only = Spy::default();
    let mut value = levels;
    render_field(
        &mut read_only,
        "levels",
        &ty,
        &mut value,
        true,
        &context(&theme, &names, bounds, None),
    );
    assert!(
        read_only.used(theme.warning),
        "the read-only list shows 150 inside 0–100: its items lost the field's range"
    );
}
