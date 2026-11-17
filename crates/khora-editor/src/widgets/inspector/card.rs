// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! The component card: a flat collapsible group per component, closed by a
//! hairline, its row actions shown only on hover so the rest reads as data.
//! See the book: design/editor.md, "Inspector (right, 320 px)".

use khora_sdk::editor_ui::{EditorState, FontFamilyHint, Icon, TextAlign, UiBuilder, UiTheme};
use khora_sdk::prelude::ecs::EntityId;

use super::display::domain_label;
use crate::widgets::{Column, Painter};
use khora_tool_ui::widgets::{
    self, group_header,
    paint::{icon_centered, tint},
    Group,
};

const HEADER_H: f32 = 32.0;

/// How a component stands against the prefab of the instance its entity
/// belongs to.
pub struct CardPrefab {
    /// How many of its fields override the prefab's.
    pub overrides: usize,
    /// Whether the instance added the component: the prefab does not give it.
    pub added: bool,
    /// Whether the prefab takes changes: an imported one (a glTF scene) does
    /// not, so the card offers no Apply.
    pub can_apply: bool,
}

impl CardPrefab {
    fn differs(&self) -> bool {
        self.added || self.overrides > 0
    }

    /// Returns the header's quiet note of it: `added`, or how many fields
    /// override.
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

/// What a card's header was asked this frame — the caller turns it into the
/// edit its component takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardAction {
    /// Take the component off the entity.
    Remove,
    /// Bring the prefab's value back — or take an added component off.
    Revert,
    /// Write the component into the prefab.
    Apply,
}

/// What a component card's header shows and offers.
pub struct CardHead<'a> {
    /// The entity holding the component.
    pub entity: EntityId,
    /// The component's name, the card's title.
    pub title: &'a str,
    /// The glyph before the title.
    pub icon: Icon,
    /// The right-aligned tag; empty for none.
    pub category: &'a str,
    /// Whether the header offers to remove the component.
    pub removable: bool,
    /// How the component stands against the entity's prefab, if it is an instance's.
    pub prefab: Option<CardPrefab>,
}

/// Renders one component as a collapsible group in `column`, `body` drawing its content;
/// returns what the header was asked, if anything.
///
/// Whether the card is open persists in `EditorState::inspector_card_open`,
/// keyed by entity and title.
pub(crate) fn render_card(
    p: &mut Painter<'_>,
    column: Column,
    head: CardHead<'_>,
    state: &mut EditorState,
    body: &mut dyn FnMut(&mut dyn UiBuilder),
) -> Option<CardAction> {
    let (ui, theme) = (&mut *p.ui, p.theme);
    let CardHead {
        entity,
        title,
        icon,
        category,
        removable,
        prefab,
    } = head;
    let Column {
        x: card_x,
        w: card_w,
    } = column;
    let mut action = None;
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

    let cat = category;
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
    if !head.contains_pointer {
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
    // Allocated after the header so their hit-rects win the click, left of
    // the category tag, and shown while the pointer is anywhere on the header.
    if head.contains_pointer {
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
                action = Some(CardAction::Remove);
            }
        }

        let can_apply = prefab.as_ref().is_some_and(|prefab| prefab.can_apply);
        if differs && can_apply {
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
                action = Some(CardAction::Apply);
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
                action = Some(CardAction::Revert);
            }
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
    action
}

/// Returns the tag shown on a component's card: the domain tag it is registered under.
///
/// A component with no domain gets no tag rather than a wrong one.
pub fn category_for(domain: Option<u8>) -> &'static str {
    domain_label(domain).unwrap_or("")
}

/// Paints a header row action, a glyph tinted on hover; returns whether it was
/// clicked.
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

#[cfg(test)]
mod tests {
    use super::category_for;
    use khora_sdk::khora_data::ecs::{AudioSource, MeshRef, RigidBody, Transform, World};

    /// The domain tag `World` registered `T` under, as `ComponentJson::domain` carries it.
    fn registered_tag<T: 'static>(world: &World) -> Option<u8> {
        world
            .component_domain(std::any::TypeId::of::<T>())
            .map(|domain| domain.index() as u8)
    }

    /// A card's tag is the domain its component is registered under.
    #[test]
    fn known_components_get_their_domain_tag() {
        let world = World::new();
        assert_eq!(category_for(registered_tag::<Transform>(&world)), "Spatial");
        assert_eq!(category_for(registered_tag::<MeshRef>(&world)), "Render");
        assert_eq!(category_for(registered_tag::<RigidBody>(&world)), "Physics");
        assert_eq!(category_for(registered_tag::<AudioSource>(&world)), "Audio");
    }

    /// A component with no registered domain must render with *no* tag rather
    /// than a wrong one — a confidently incorrect label is worse than none.
    #[test]
    fn components_with_no_domain_get_no_tag() {
        assert_eq!(category_for(None), "");
        assert_eq!(category_for(Some(u8::MAX)), "");
    }
}
