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

//! What Play shows of scripts: a behavior's state, read-only, only while
//! the game runs; a system's own entity, named after its system in the
//! hierarchy, its card read-only.

use khora_sdk::editor_ui::DeclaredCardKind;
use khora_sdk::khora_data::ecs::systems::script_systems::singleton_id;

use super::*;

/// A sentry running `Guard`, its state companion held, and its world.
fn sentry() -> (GameWorld, EntityId) {
    let mut world = GameWorld::new();
    let (behavior, companion) = guard(&mut world);
    let sentry = world.spawn((Transform::identity(), Name::new("Sentry")));
    attach(&mut world, sentry, behavior);
    attach(&mut world, sentry, companion);
    set(
        &mut world,
        sentry,
        companion,
        "state",
        ScriptValue::Str("Patrol".to_owned()),
    );
    (world, sentry)
}

/// The system `Clock`'s own entity, as the script systems spawn it: no
/// `Name`, known by `system:Clock`, holding the system's component.
fn clock_entity(world: &mut GameWorld) -> EntityId {
    let key = clock(world);
    let entity = world.spawn(());
    world
        .inner_world_mut()
        .set_persistent_id(entity, singleton_id("Clock"));
    attach(world, entity, key);
    entity
}

/// **A behavior's state is not shown while editing**: an edited scene runs
/// no script, and what it held is the engine's, not the author's.
#[test]
fn a_behaviors_state_card_is_hidden_while_editing() {
    let (world, sentry) = sentry();

    let inspected = inspect(&world, sentry, PlayMode::Editing);

    assert!(card(&inspected, "Guard").is_some(), "the behavior's card");
    assert!(
        card(&inspected, "Guard#state").is_none(),
        "no state card while editing"
    );
}

/// **A behavior's state is shown read-only while the game runs** — paused
/// too — with what the engine keeps of the instance; the behavior's own
/// card stays editable, an edit in Play changing the live value.
#[test]
fn a_behaviors_state_card_is_read_only_while_playing() {
    let (world, sentry) = sentry();

    for mode in [PlayMode::Playing, PlayMode::Paused] {
        let inspected = inspect(&world, sentry, mode);

        let state = card_of(&inspected, "Guard#state");
        assert_eq!(state.kind, DeclaredCardKind::BehaviorState, "{mode:?}");
        assert!(state.read_only, "{mode:?}: the engine writes it");
        assert_eq!(
            state
                .fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            vec!["state", "timers", "fault"],
            "{mode:?}"
        );
        assert_eq!(
            state
                .fields
                .iter()
                .find(|field| field.name == "state")
                .map(|field| field.value.clone()),
            Some(ScriptValue::Str("Patrol".to_owned())),
            "{mode:?}"
        );
        assert!(
            !card_of(&inspected, "Guard").read_only,
            "{mode:?}: the behavior's fields stay editable"
        );
    }
}

/// **A system's card is read-only while playing**: its fields are its own
/// entity's, written by the engine.
#[test]
fn a_systems_card_is_read_only_while_playing() {
    let mut world = GameWorld::new();
    let entity = clock_entity(&mut world);

    let inspected = inspect(&world, entity, PlayMode::Playing);
    let card = card_of(&inspected, "Clock");

    assert_eq!(card.kind, DeclaredCardKind::System);
    assert_eq!(card.module.as_deref(), Some(CLOCK_MODULE));
    assert!(card.read_only);
}

/// **A system's entity is named after its system in the hierarchy**: it
/// has no `Name`, and `Entity N` would say nothing of what it is.
#[test]
fn a_systems_entity_is_named_after_its_system_in_the_hierarchy() {
    let mut world = GameWorld::new();
    let entity = clock_entity(&mut world);
    world.spawn((Transform::identity(), Name::new("Floor")));
    let mut state = EditorState {
        play_mode: PlayMode::Playing,
        ..EditorState::default()
    };

    crate::ops::extract_scene_tree(&world, &mut state);

    let node = state
        .scene_roots
        .iter()
        .find(|node| node.entity == entity)
        .expect("the system's entity is in the hierarchy");
    assert_eq!(node.name, "system:Clock");
    assert!(
        state.scene_roots.iter().any(|node| node.name == "Floor"),
        "a named entity keeps its name"
    );
}
