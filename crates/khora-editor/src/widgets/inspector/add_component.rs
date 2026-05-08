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

/// Components that are inherent to a vessel (auto-managed by the engine
/// or surfaced elsewhere) — never offered in the "+ Add Component" menu
/// nor rendered as a card. `Name` lives in the inspector header instead.
pub const INHERENT_COMPONENTS: &[&str] = &[
    "GlobalTransform",
    "Parent",
    "Children",
    "HandleComponent<GpuMesh>",
    "Name",
];

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
