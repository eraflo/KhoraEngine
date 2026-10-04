// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Component group — one collapsible section per component on the entity.
//!
//! Deliberately **flat**: a chevron, an icon, a name, a category tag, and a
//! hairline. It used to be a bordered, filled card per component, which turned
//! a five-component entity into boxes inside boxes inside a panel — the
//! structure stopped being readable exactly when there was enough of it to
//! need reading. Hierarchy now comes from type and spacing.
//!
//! The row actions (enable toggle, remove) appear on hover. They are rare and
//! destructive-adjacent; the resting state should read as data, not as a
//! toolbar.

use khora_sdk::editor_ui::{
    EditorState, FontFamilyHint, Icon, PrefabApplyScope, PropertyEdit, TextAlign, UiBuilder,
    UiTheme,
};
use khora_sdk::prelude::ecs::EntityId;
use khora_tool_ui::widgets::{
    self, group_header,
    paint::{icon_centered, tint},
    Group,
};

const HEADER_H: f32 = 32.0;

/// How a component stands against the prefab of the instance its entity
/// belongs to.
pub struct CardPrefab<'a> {
    /// How many of its fields override the prefab's.
    pub overrides: usize,
    /// Whether the instance added the component: the prefab does not give it.
    pub added: bool,
    /// The prefab's value of it, where the prefab gives it.
    pub prefab_value: Option<&'a serde_json::Value>,
}

impl CardPrefab<'_> {
    fn differs(&self) -> bool {
        self.added || self.overrides > 0
    }

    /// The header's quiet note of it: `added`, or how many fields override.
    fn note(&self) -> Option<String> {
        if self.added {
            Some("added".to_owned())
        } else if self.overrides == 1 {
            Some("1 override".to_owned())
        } else if self.overrides > 1 {
            Some(format!("{} overrides", self.overrides))
        } else {
            None
        }
    }
}

/// A row action on the header: a square hit-rect with its glyph, tinted on
/// hover. Returns whether it was clicked.
#[allow(clippy::too_many_arguments)]
fn header_action(
    ui: &mut dyn UiBuilder,
    id: &str,
    rect: [f32; 4],
    glyph: Icon,
    accent: [f32; 4],
    theme: &UiTheme,
) -> bool {
    let hit = ui.interact_rect(id, rect);
    if hit.hovered {
        widgets::fill(ui, rect, tint(accent, 0.14), theme.radius_sm);
    }
    icon_centered(
        ui,
        rect,
        glyph,
        12.0,
        if hit.hovered {
            accent
        } else {
            theme.text_muted
        },
    );
    hit.clicked
}

/// The domain a component belongs to, shown as a small right-aligned tag.
///
/// Derived from the type name so a newly-added component is tagged without
/// anyone having to register it anywhere; unknown components simply carry no
/// tag rather than a wrong one.
fn category_for(title: &str) -> &'static str {
    match title {
        "Transform" | "GlobalTransform" | "Parent" | "Children" => "spatial",
        "MeshRef" | "MaterialRef" | "Visibility" | "Camera" | "Light" => "render",
        "RigidBody" | "Collider" | "Velocity" => "physics",
        "AudioSource" | "AudioListener" => "audio",
        "Name" | "Tags" => "meta",
        _ => "",
    }
}

/// Renders one component as a flat collapsible group and calls `body` for its
/// content. Open/closed state persists in `EditorState::inspector_card_open`,
/// keyed by entity index + component title.
#[allow(clippy::too_many_arguments)]
pub fn render_card(
    ui: &mut dyn UiBuilder,
    entity: EntityId,
    title: &str,
    icon: Icon,
    enabled: Option<bool>,
    removable: bool,
    prefab: Option<CardPrefab<'_>>,
    card_x: f32,
    card_w: f32,
    theme: &UiTheme,
    state: &mut EditorState,
    body: &mut dyn FnMut(&mut dyn UiBuilder),
) {
    // Keyed on index AND generation: entity slots are recycled, so a key built
    // from the index alone would hand a freshly spawned entity the collapse
    // state of whatever previously occupied that slot.
    let card_id = format!("{}.{}::{}", entity.index, entity.generation, title);
    let open = *state
        .inspector_card_open
        .entry(card_id.clone())
        .or_insert(true);

    let y = ui.cursor_pos()[1];
    let header = [card_x, y, card_w, HEADER_H];

    let cat = category_for(title);
    let spec = Group::new(title, icon).open(open).category(cat);
    let head = group_header(ui, theme, header, &format!("card-hdr-{card_id}"), spec);

    let tag_w = if cat.is_empty() {
        0.0
    } else {
        ui.measure_text(
            &cat.to_uppercase(),
            theme.font_size_caption - 1.5,
            FontFamilyHint::Monospace,
        )[0] + 10.0
    };
    let mut right = card_x + card_w - 12.0 - tag_w;

    // ── How it stands against the prefab ──
    // A quiet note left of the category tag, at rest; the actions take its
    // place on hover, so the header never holds both.
    let differs = prefab.as_ref().is_some_and(CardPrefab::differs);
    if !head.hovered {
        if let Some(note) = prefab.as_ref().and_then(CardPrefab::note) {
            let size = theme.font_size_caption - 1.5;
            ui.paint_text_styled(
                [right, y + (HEADER_H - size) * 0.5 - 1.0],
                &note,
                size,
                theme.primary,
                FontFamilyHint::Monospace,
                TextAlign::Right,
            );
        }
    }

    // ── Row actions, revealed on hover ──
    // Allocated *after* the header so their hit-rects win the click, and laid
    // out to the left of the category tag so they never sit on top of it.
    if head.hovered {
        let size = 22.0;
        let top = y + (HEADER_H - size) * 0.5;

        if removable {
            let rect = [right - size, top, size, size];
            right -= size + 2.0;
            if header_action(
                ui,
                &format!("card-rm-{card_id}"),
                rect,
                Icon::Trash,
                theme.error,
                theme,
            ) {
                state.pending_edits.push(PropertyEdit::RemoveComponent {
                    entity,
                    type_name: title.to_string(),
                });
            }
        }

        if let Some(prefab) = prefab.as_ref().filter(|_| differs) {
            let rect = [right - size, top, size, size];
            right -= size + 2.0;
            if header_action(
                ui,
                &format!("card-apply-{card_id}"),
                rect,
                Icon::ApplyToPrefab,
                theme.primary,
                theme,
            ) {
                state.pending_prefab_apply = Some((
                    entity,
                    PrefabApplyScope::Component {
                        type_name: title.to_string(),
                    },
                ));
            }
            let rect = [right - size, top, size, size];
            if header_action(
                ui,
                &format!("card-revert-{card_id}"),
                rect,
                Icon::Revert,
                theme.primary,
                theme,
            ) {
                // Added by the instance: reverting takes it off. Otherwise
                // the prefab's value comes back whole.
                match prefab.prefab_value {
                    Some(value) if !prefab.added => {
                        state.pending_edits.push(PropertyEdit::SetComponentJson {
                            entity,
                            type_name: title.to_string(),
                            value: value.clone(),
                        });
                    }
                    _ => state.pending_edits.push(PropertyEdit::RemoveComponent {
                        entity,
                        type_name: title.to_string(),
                    }),
                }
            }
        }

        // The component enable/disable switch used to be painted here. It had
        // no `interact_rect` at all, so it was a picture of a switch; and every
        // caller passed `enabled: None`, so it never even appeared.
        //
        // Turning a component off only means something once the ECS honours it
        // — every query that consumes the component has to check the flag, or
        // it is decorative. That is a data-layer feature, not a card one.
        debug_assert!(
            enabled.is_none(),
            "component enable/disable is not implemented; \
             see the component-activation work before passing Some(_)"
        );
    }

    if head.clicked {
        state.inspector_card_open.insert(card_id, !open);
    }

    ui.spacing(HEADER_H + 2.0);

    if open {
        ui.indent("card-body", &mut |ui_b| body(ui_b));
        ui.spacing(8.0);
    }

    // A hairline closes the group — the only rule the flat design needs.
    let end_y = ui.cursor_pos()[1];
    ui.paint_line(
        [card_x, end_y],
        [card_x + card_w, end_y],
        theme.separator,
        1.0,
    );
    ui.spacing(6.0);
}

#[cfg(test)]
mod tests {
    use super::category_for;

    #[test]
    fn known_components_get_their_domain_tag() {
        assert_eq!(category_for("Transform"), "spatial");
        assert_eq!(category_for("MeshRef"), "render");
        assert_eq!(category_for("RigidBody"), "physics");
        assert_eq!(category_for("AudioSource"), "audio");
    }

    /// An unregistered component must render with *no* tag rather than a
    /// wrong one — a confidently incorrect label is worse than none.
    #[test]
    fn unknown_components_get_no_tag() {
        assert_eq!(category_for("PlayerController"), "");
        assert_eq!(category_for(""), "");
    }
}
