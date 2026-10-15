// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! "+ Add Component" menu — bucketed by `SemanticDomain` (live World).
//!
//! Lists what `ops::addable_components` offers the entity — the Rust
//! components of the inventory and the ones a script declares, minus what
//! the entity holds — and groups it by domain tag.

use khora_sdk::editor_ui::{
    AddableComponent, ComponentOrigin, EditorState, InspectedEntity, UiBuilder,
};
use khora_sdk::prelude::ecs::EntityId;

use super::display::category_label_for_tag;

/// Components the editor renders somewhere other than as an inspector card,
/// so offering them in "+ Add Component" would be redundant.
///
/// This is a **presentation** concern, not an authorship one: `Name` is very
/// much the author's data, it simply lives in the inspector header. Whether a
/// component may be authored at all is answered by
/// [`ComponentProvenance`](khora_sdk::khora_data::ecs::ComponentProvenance),
/// which is declared on the component itself — so engine-written types are
/// excluded by construction, including ones defined outside this workspace.
pub const SURFACED_ELSEWHERE: &[&str] = &["Name"];

/// `SemanticDomain::Ui` as the small integer tag the editor carries in
/// [`ComponentJson::domain`] (see `ops::scene_tree::domain_tag`).
///
/// The `Ui*` family drives the in-world Taffy UI, which is authored in a
/// canvas, not on a 3D scene entity — dropping a `UiNode` on a mesh yields a
/// component nothing lays out. Scene mode therefore hides the whole domain.
/// This is a **workspace** filter, not an authorship one: the day a Canvas
/// workspace ships, it shows this domain and hides the others rather than
/// removing the rule.
const UI_DOMAIN_TAG: u8 = 4;

/// Whether the inspector should treat `type_name` as the author's own data —
/// offering it in "+ Add Component" and rendering it as an editable card.
///
/// The single source of truth for both surfaces, so the menu and the card list
/// cannot drift apart. Returns `false` for anything the engine writes
/// (`GlobalTransform`, `Children`, `PhysicsDebugData`…), for tool-written
/// components nobody adds by hand (`Parent`), for the ones rendered
/// elsewhere, and for unregistered types.
pub fn is_author_facing(type_name: &str) -> bool {
    if SURFACED_ELSEWHERE.contains(&type_name) {
        return false;
    }
    khora_sdk::khora_data::scene::provenance_of(type_name).is_some_and(|p| p.is_hand_authorable())
}

/// The file name of a module path, as a menu entry names its script.
fn module_file(module: &str) -> &str {
    module.rsplit(['/', '\\']).next().unwrap_or(module)
}

/// Render the "+ Add Component" menu button from what the entity is offered
/// ([`EditorState::addable_components`]): grouped by domain, a script's
/// entry marked with its `.erg`. Selecting one queues
/// `EditorState::pending_add_component` for the next frame.
pub fn render_add_component(
    ui: &mut dyn UiBuilder,
    entity: EntityId,
    _inspected: &InspectedEntity,
    state: &mut EditorState,
) {
    let mut buckets: std::collections::BTreeMap<Option<u8>, Vec<AddableComponent>> =
        std::collections::BTreeMap::new();
    for entry in &state.addable_components {
        let tag = entry.domain.map(|domain| domain.index() as u8);
        if tag == Some(UI_DOMAIN_TAG) {
            continue;
        }
        buckets.entry(tag).or_default().push(entry.clone());
    }
    // Rust components first, then a script's, each set by name.
    for entries in buckets.values_mut() {
        entries.sort_by(|a, b| {
            (a.origin != ComponentOrigin::Rust, &a.name)
                .cmp(&(b.origin != ComponentOrigin::Rust, &b.name))
        });
    }

    let pending: std::cell::Cell<Option<String>> = std::cell::Cell::new(None);
    ui.menu_button("+ Add Component", &mut |ui_m| {
        // "Other" — no domain known — comes last.
        let ordered = buckets
            .iter()
            .filter(|(tag, _)| tag.is_some())
            .chain(buckets.iter().filter(|(tag, _)| tag.is_none()));
        for (tag, entries) in ordered {
            let label = category_label_for_tag(*tag).to_string();
            ui_m.menu_button(&label, &mut |ui_s| {
                for entry in entries {
                    let clicked = match &entry.origin {
                        ComponentOrigin::Rust => ui_s.button(&entry.name),
                        ComponentOrigin::Script { module } => {
                            let clicked = ui_s.button(&format!(
                                "{}   \u{00B7} {}",
                                entry.name,
                                module_file(module)
                            ));
                            ui_s.tooltip_for_last(&format!("Declared by {module}"));
                            clicked
                        }
                    };
                    if clicked {
                        pending.set(Some(entry.name.clone()));
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
