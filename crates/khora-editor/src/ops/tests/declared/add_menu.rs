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

//! "+ Add Component" lists the World registry's author-facing components:
//! the Rust ones and the ones a script declares — its components and its
//! behaviors, each marked with its `.erg` — never what the engine writes,
//! what no module declares, or what the entity already holds.

use khora_sdk::editor_ui::{AddableComponent, ComponentOrigin};

use super::*;
use crate::ops::addable_components;

/// A world declaring every kind of script component — a component, a
/// behavior and its state, a system, an orphan — and an entity holding
/// none of them.
fn scripted() -> (GameWorld, EntityId) {
    let mut world = GameWorld::new();
    lantern(&mut world);
    guard(&mut world);
    clock(&mut world);
    relic(&mut world);
    let crate_ = world.spawn((Transform::identity(), Name::new("Crate")));
    (world, crate_)
}

/// The entry offering `name`, if one does.
fn offered<'a>(entries: &'a [AddableComponent], name: &str) -> Option<&'a AddableComponent> {
    entries.iter().find(|entry| entry.name == name)
}

fn names(entries: &[AddableComponent]) -> Vec<&str> {
    entries.iter().map(|entry| entry.name.as_str()).collect()
}

/// **A script's components and behaviors are offered, marked with their
/// module** and grouped by their domain.
#[test]
fn the_add_menu_offers_script_components_and_behaviors_marked_with_their_module() {
    let (world, crate_) = scripted();

    let entries = addable_components(&world, crate_);

    assert_eq!(
        offered(&entries, "Lantern"),
        Some(&AddableComponent {
            name: "Lantern".to_owned(),
            domain: Some(SemanticDomain::Render),
            origin: ComponentOrigin::Script {
                module: LANTERN_MODULE.to_owned(),
            },
        }),
        "offered: {:?}",
        names(&entries)
    );
    assert_eq!(
        offered(&entries, "Guard"),
        Some(&AddableComponent {
            name: "Guard".to_owned(),
            domain: Some(SemanticDomain::Script),
            origin: ComponentOrigin::Script {
                module: GUARD_MODULE.to_owned(),
            },
        }),
        "offered: {:?}",
        names(&entries)
    );
}

/// **What the engine writes and what no module declares are not offered**:
/// a behavior's state, a system's fields, an orphaned component.
#[test]
fn the_add_menu_offers_no_state_companion_no_system_and_no_orphan() {
    let (world, crate_) = scripted();

    let entries = addable_components(&world, crate_);
    let offered = names(&entries);

    for kept_out in ["Guard#state", "Clock", "Relic"] {
        assert!(
            !offered.contains(&kept_out),
            "`{kept_out}` is not offered: {offered:?}"
        );
    }
}

/// **The Rust components are offered beside the declared ones**, marked as
/// Rust — but not what the inspector shows elsewhere (`Name`) nor what the
/// engine derives (`GlobalTransform`).
#[test]
fn the_add_menu_lists_rust_components_beside_the_declared_ones() {
    let (world, crate_) = scripted();

    let entries = addable_components(&world, crate_);

    let light = offered(&entries, "Light")
        .unwrap_or_else(|| panic!("`Light` is offered: {:?}", names(&entries)));
    assert_eq!(light.origin, ComponentOrigin::Rust);
    assert_eq!(light.domain, Some(SemanticDomain::Render));
    assert!(
        offered(&entries, "Lantern").is_some(),
        "beside the declared"
    );
    for kept_out in ["Name", "GlobalTransform"] {
        assert!(
            offered(&entries, kept_out).is_none(),
            "`{kept_out}` is not offered: {:?}",
            names(&entries)
        );
    }
}

/// **Nothing the entity holds is offered** — a declared component or a Rust
/// one.
#[test]
fn the_add_menu_offers_nothing_the_entity_holds() {
    let mut world = GameWorld::new();
    let key = lantern(&mut world);
    let (behavior, _) = guard(&mut world);
    let lamp = world.spawn((Transform::identity(), Name::new("Lamp")));
    attach(&mut world, lamp, key);
    attach(&mut world, lamp, behavior);

    let entries = addable_components(&world, lamp);
    let offered = names(&entries);

    for held in ["Lantern", "Guard", "Transform"] {
        assert!(
            !offered.contains(&held),
            "`{held}` is held, not offered: {offered:?}"
        );
    }
    assert!(offered.contains(&"Light"), "{offered:?}");
}
