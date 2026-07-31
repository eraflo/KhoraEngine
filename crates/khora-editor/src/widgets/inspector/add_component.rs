// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! "+ Add Component" menu — bucketed by `SemanticDomain` (live World).
//!
//! Walks the inventory of `ComponentRegistration` to enumerate every
//! component the engine knows about, filters out inherent-on-vessel
//! components plus those already on the entity, and groups what's left
//! by domain tag.

use khora_sdk::editor_ui::{EditorState, InspectedEntity, UiBuilder};
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
/// [`ComponentJson::domain`] (see `ops::domain_tag`).
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
/// components nobody adds by hand (`Parent`, `Prefab`), for the ones rendered
/// elsewhere, and for unregistered types.
pub fn is_author_facing(type_name: &str) -> bool {
    if SURFACED_ELSEWHERE.contains(&type_name) {
        return false;
    }
    khora_sdk::khora_data::scene::provenance_of(type_name)
        .is_some_and(|p| p.is_hand_authorable())
}

/// Render the "+ Add Component" menu button. Selecting a component
/// queues `EditorState::pending_add_component` for the next frame.
pub fn render_add_component(
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
        if !is_author_facing(reg.type_name) {
            continue;
        }
        if already_present.contains(reg.type_name) {
            continue;
        }
        let domain_tag = state.component_domain_registry.get(reg.type_name).copied();
        if domain_tag == Some(UI_DOMAIN_TAG) {
            continue;
        }

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
