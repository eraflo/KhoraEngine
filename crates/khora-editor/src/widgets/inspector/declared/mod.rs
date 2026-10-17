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

//! The cards of components a script declared: the same group as a Rust
//! component's, its rows drawn by Ergon type rather than from JSON.
//!
//! Under the title, one quiet line says where the component comes from —
//! the `.erg` declaring it, that the engine writes it, or that no script
//! declares it any more. The category tag names the part it plays:
//! `script`, `behavior`, `state`, `system`.

mod fields;
mod hints;
mod ranged;

use std::collections::HashMap;

use khora_sdk::editor_ui::{
    DeclaredCard, DeclaredCardKind, EditorState, FontFamilyHint, InspectedEntity, InspectedPrefab,
    PrefabApplyScope, PropertyEdit, SceneNode, TextAlign, UiBuilder, UiTheme,
};
use khora_sdk::khora_core::script::ScriptValue;
use khora_sdk::prelude::ecs::EntityId;

use super::card::{render_card, CardAction, CardPrefab};
use super::display::icon_for_domain_tag;
use super::walker::{FieldAction, Marks};
use fields::{render_field, FieldContext};

/// The tag a card's header carries for the part its component plays.
pub fn kind_tag(kind: DeclaredCardKind) -> &'static str {
    match kind {
        DeclaredCardKind::Component => "script",
        DeclaredCardKind::Behavior => "behavior",
        DeclaredCardKind::BehaviorState => "state",
        DeclaredCardKind::System => "system",
    }
}

/// The line under a card's title: where the component comes from.
pub fn provenance_line(card: &DeclaredCard) -> String {
    match (&card.module, card.kind) {
        (_, DeclaredCardKind::BehaviorState) => "Written by the engine while playing".to_owned(),
        (_, DeclaredCardKind::System) => "A system's fields — written by its script".to_owned(),
        (None, _) => format!(
            "No script declares {} any more — its data is kept",
            card.name
        ),
        (Some(module), _) => module.clone(),
    }
}

/// Every entity of the scene tree by id, as an entity slot names it.
pub fn scene_names(roots: &[SceneNode]) -> HashMap<EntityId, String> {
    fn walk(nodes: &[SceneNode], names: &mut HashMap<EntityId, String>) {
        for node in nodes {
            names.insert(node.entity, node.name.clone());
            walk(&node.children, names);
        }
    }
    let mut names = HashMap::new();
    walk(roots, &mut names);
    names
}

/// The prefab's fields of the declared component `name`.
fn prefab_fields<'p>(
    prefab: &'p InspectedPrefab,
    name: &str,
) -> Option<&'p [(String, ScriptValue)]> {
    prefab
        .prefab_declared
        .iter()
        .find(|(component, _)| component == name)
        .map(|(_, fields)| fields.as_slice())
}

/// The prefab's value of the field `field` of `name`.
fn prefab_field<'p>(
    prefab: &'p InspectedPrefab,
    name: &str,
    field: &str,
) -> Option<&'p ScriptValue> {
    prefab_fields(prefab, name)?
        .iter()
        .find(|(known, _)| known == field)
        .map(|(_, value)| value)
}

/// The edits that bring `name` back to its prefab's: each overridden field
/// set back — or, where the instance added it, the component taken off.
fn reverted(entity: EntityId, name: &str, prefab: &InspectedPrefab) -> Vec<PropertyEdit> {
    let added = prefab
        .components
        .iter()
        .any(|component| component.type_name == name && component.added);
    if added {
        return vec![PropertyEdit::RemoveDeclared {
            entity,
            component: name.to_owned(),
        }];
    }
    prefab
        .fields_of(name)
        .iter()
        .filter_map(|path| {
            let field = path.first()?;
            Some(PropertyEdit::SetDeclaredField {
                entity,
                component: name.to_owned(),
                field: field.clone(),
                value: prefab_field(prefab, name, field)?.clone(),
            })
        })
        .collect()
}

/// The edits that restore `name`, which the instance removed: added again,
/// each field given the prefab's value.
pub fn restored(entity: EntityId, name: &str, prefab: &InspectedPrefab) -> Vec<PropertyEdit> {
    let Some(fields) = prefab_fields(prefab, name) else {
        return Vec::new();
    };
    let mut edits = vec![PropertyEdit::AddDeclared {
        entity,
        component: name.to_owned(),
    }];
    edits.extend(
        fields
            .iter()
            .map(|(field, value)| PropertyEdit::SetDeclaredField {
                entity,
                component: name.to_owned(),
                field: field.clone(),
                value: value.clone(),
            }),
    );
    edits
}

/// Draws the declared cards of `inspected` and gathers their edits.
#[allow(clippy::too_many_arguments)]
pub fn render_declared_cards(
    ui: &mut dyn UiBuilder,
    inspected: &InspectedEntity,
    names: &HashMap<EntityId, String>,
    card_x: f32,
    card_w: f32,
    theme: &UiTheme,
    state: &mut EditorState,
    edits: &mut Vec<PropertyEdit>,
) {
    let entity = inspected.entity;
    let prefab = inspected.prefab.as_ref();
    for card in &inspected.declared {
        let overridden: Vec<Vec<String>> = prefab
            .map(|prefab| prefab.fields_of(&card.name).to_vec())
            .unwrap_or_default();
        let card_prefab = prefab.map(|prefab| CardPrefab {
            overrides: overridden.len(),
            added: prefab
                .components
                .iter()
                .any(|component| component.type_name == card.name && component.added),
        });
        let icon = icon_for_domain_tag(Some(card.domain.index() as u8));
        let mut marks = Marks::new(&overridden);
        let mut changed: Vec<(String, ScriptValue)> = Vec::new();
        let action = render_card(
            ui,
            entity,
            &card.name,
            icon,
            kind_tag(card.kind),
            None,
            !card.read_only,
            card_prefab.filter(|_| !card.read_only),
            card_x,
            card_w,
            theme,
            state,
            &mut |body| {
                render_provenance(body, card, theme);
                for (at, field) in card.fields.iter().enumerate() {
                    if let Some(header) = hints::section_header(field) {
                        ranged::render_header(body, theme, &header, at == 0);
                    }
                    let mut value = field.value.clone();
                    let top = body.cursor_pos();
                    let cx = FieldContext {
                        theme,
                        names,
                        id: format!("declared.{}.{}.{}", entity.index, card.name, field.name),
                        range: field.hints.range,
                        tooltip: field.hints.tooltip.as_deref(),
                    };
                    if render_field(
                        body,
                        &field.name,
                        &field.ty,
                        &mut value,
                        card.read_only,
                        &cx,
                    ) {
                        changed.push((field.name.clone(), value));
                    }
                    marks.mark_field(body, &field.name, top, theme);
                }
            },
        );
        edits.extend(
            changed
                .into_iter()
                .map(|(field, value)| PropertyEdit::SetDeclaredField {
                    entity,
                    component: card.name.clone(),
                    field,
                    value,
                }),
        );
        for field_action in marks.actions.drain(..) {
            match field_action {
                FieldAction::Revert(path) => {
                    let value = path
                        .first()
                        .and_then(|field| Some((field, prefab_field(prefab?, &card.name, field)?)));
                    if let Some((field, value)) = value {
                        edits.push(PropertyEdit::SetDeclaredField {
                            entity,
                            component: card.name.clone(),
                            field: field.clone(),
                            value: value.clone(),
                        });
                    }
                }
                FieldAction::Apply(path) => {
                    state.pending_prefab_apply = Some((
                        entity,
                        PrefabApplyScope::Field {
                            type_name: card.name.clone(),
                            path,
                        },
                    ));
                }
            }
        }
        match action {
            Some(CardAction::Remove) => edits.push(PropertyEdit::RemoveDeclared {
                entity,
                component: card.name.clone(),
            }),
            Some(CardAction::Revert) => {
                if let Some(prefab) = prefab {
                    edits.extend(reverted(entity, &card.name, prefab));
                }
            }
            Some(CardAction::Apply) => {
                state.pending_prefab_apply = Some((
                    entity,
                    PrefabApplyScope::Component {
                        type_name: card.name.clone(),
                    },
                ));
            }
            None => {}
        }
    }
}

/// The quiet line under a card's title. An orphan's reads as a warning: the
/// data stays, but no script will run it.
fn render_provenance(ui: &mut dyn UiBuilder, card: &DeclaredCard, theme: &UiTheme) {
    let text = provenance_line(card);
    let size = theme.font_size_caption;
    let color = if card.orphaned && card.kind != DeclaredCardKind::System {
        theme.warning
    } else {
        theme.text_muted
    };
    let family = if card.module.is_some()
        && matches!(
            card.kind,
            DeclaredCardKind::Component | DeclaredCardKind::Behavior
        ) {
        FontFamilyHint::Monospace
    } else {
        FontFamilyHint::Proportional
    };
    let rect = ui.allocate_size([ui.available_width(), size + 6.0]);
    ui.paint_text_styled(
        [rect[0], rect[1] + 1.0],
        &text,
        size,
        color,
        family,
        TextAlign::Left,
    );
    if card.orphaned {
        ui.tooltip_for_last(
            "Its script was removed or renamed. Remove the component, or declare it again.",
        );
    }
}

#[cfg(test)]
mod tests;
