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

use khora_sdk::editor_ui::{EditorState, Icon, PropertyEdit, UiBuilder, UiTheme};
use khora_sdk::prelude::ecs::EntityId;
use khora_tool_ui::widgets::{
    self, group_header,
    paint::{icon_centered, tint},
    Group,
};

const HEADER_H: f32 = 32.0;

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

    // ── Row actions, revealed on hover ──
    // Allocated *after* the header so their hit-rects win the click, and laid
    // out to the left of the category tag so they never sit on top of it.
    if head.hovered {
        let tag_w = if cat.is_empty() {
            0.0
        } else {
            ui.measure_text(
                &cat.to_uppercase(),
                theme.font_size_caption - 1.5,
                khora_sdk::editor_ui::FontFamilyHint::Monospace,
            )[0] + 10.0
        };
        let mut right = card_x + card_w - 12.0 - tag_w;

        if removable {
            let size = 22.0;
            let rect = [right - size, y + (HEADER_H - size) * 0.5, size, size];
            let hit = ui.interact_rect(&format!("card-rm-{card_id}"), rect);
            if hit.hovered {
                widgets::fill(ui, rect, tint(theme.error, 0.14), theme.radius_sm);
            }
            icon_centered(
                ui,
                rect,
                Icon::Trash,
                12.0,
                if hit.hovered {
                    theme.error
                } else {
                    theme.text_muted
                },
            );
            if hit.clicked {
                state.pending_edits.push(PropertyEdit::RemoveComponent {
                    entity,
                    type_name: title.to_string(),
                });
            }
            right -= size + 4.0;
        }

        if let Some(on) = enabled {
            let (tw, th) = (26.0, 14.0);
            let tx = right - tw;
            let ty = y + (HEADER_H - th) * 0.5;
            widgets::fill(
                ui,
                [tx, ty, tw, th],
                if on {
                    theme.primary
                } else {
                    theme.surface_active
                },
                th * 0.5,
            );
            let knob = if on { tx + tw - 7.0 } else { tx + 7.0 };
            ui.paint_circle_filled(
                [knob, ty + th * 0.5],
                5.0,
                if on {
                    theme.text_inverse
                } else {
                    theme.text_dim
                },
            );
        }
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
