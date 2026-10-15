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

//! An edit of a declared field while the game runs changes the live value,
//! and Stop puts back the value the scene had when Play started.

use khora_sdk::editor_ui::PropertyEdit;
use khora_sdk::khora_core::script::{ErgonType, ScriptValue};
use khora_sdk::khora_data::ecs::{
    ComponentKey, ComponentProvenance, FieldKind, PackedField, PackedLayout, RuntimeComponentDecl,
    SemanticDomain,
};

use super::*;

/// Registers `Lantern { float glow = 1.0; }`, as `props/lantern.erg`
/// declares it.
fn declare_lantern(world: &mut GameWorld) -> ComponentKey {
    let inner = world.inner_world_mut();
    let key = inner
        .register_runtime_component(RuntimeComponentDecl {
            name: "Lantern".to_owned(),
            domain: SemanticDomain::Render,
            provenance: ComponentProvenance::Authored,
            resumable: false,
            layout: PackedLayout::typed(
                vec![PackedField {
                    name: "glow".to_owned(),
                    kind: FieldKind::Float,
                    default: ScriptValue::Float(1.0),
                }],
                vec![ErgonType::Float],
            )
            .expect("a valid layout"),
        })
        .expect("Lantern registers");
    inner.set_declared_by(key, Some("props/lantern.erg".to_owned()));
    key
}

fn glow(world: &GameWorld, entity: EntityId, key: ComponentKey) -> Option<ScriptValue> {
    world
        .inner_world()
        .declared_row(entity, key)?
        .into_iter()
        .find(|(field, _)| field == "glow")
        .map(|(_, value)| value)
}

/// **An edit in Play changes the live value; Stop restores the scene's.**
#[test]
fn a_declared_field_edited_in_play_is_live_and_stop_restores_it() {
    let mut world = GameWorld::new();
    let key = declare_lantern(&mut world);
    let lamp = world.spawn((Transform::identity(), Name::new("Lamp")));
    world
        .inner_world_mut()
        .add_runtime_component(lamp, key, &ScriptValue::Unit)
        .expect("Lantern attaches");
    world
        .inner_world_mut()
        .mark_authored(lamp)
        .expect("authored");
    let state = Arc::new(Mutex::new(EditorState::default()));

    apply_play(&mut world, &state);
    {
        let mut state = state.lock().expect("the editor state");
        state.push_edit(PropertyEdit::SetDeclaredField {
            entity: lamp,
            component: "Lantern".to_owned(),
            field: "glow".to_owned(),
            value: ScriptValue::Float(7.0),
        });
        crate::ops::apply_edits(&mut world, &mut state);
    }
    assert_eq!(
        glow(&world, lamp, key),
        Some(ScriptValue::Float(7.0)),
        "the edit changes the live value"
    );

    apply_stop(&mut world, &state);
    let lamp = world
        .iter_entities()
        .find(|entity| {
            world
                .get_component::<Name>(*entity)
                .is_some_and(|name| name.as_str() == "Lamp")
        })
        .expect("the lamp is back");
    assert_eq!(
        glow(&world, lamp, key),
        Some(ScriptValue::Float(1.0)),
        "Stop restores the value Play started from"
    );
}

/// `Fuse { Duration wait = 1s; Angle turn = 0deg; }`, as `game/fuse.erg`
/// declares it.
fn declare_fuse(world: &mut GameWorld) -> ComponentKey {
    let inner = world.inner_world_mut();
    let key = inner
        .register_runtime_component(RuntimeComponentDecl {
            name: "Fuse".to_owned(),
            domain: SemanticDomain::Script,
            provenance: ComponentProvenance::Authored,
            resumable: false,
            layout: PackedLayout::typed(
                vec![
                    PackedField {
                        name: "wait".to_owned(),
                        kind: FieldKind::of(&ErgonType::Duration),
                        default: ScriptValue::Float(1.0),
                    },
                    PackedField {
                        name: "turn".to_owned(),
                        kind: FieldKind::of(&ErgonType::Angle),
                        default: ScriptValue::Float(0.0),
                    },
                ],
                vec![ErgonType::Duration, ErgonType::Angle],
            )
            .expect("a valid layout"),
        })
        .expect("Fuse registers");
    inner.set_declared_by(key, Some("game/fuse.erg".to_owned()));
    key
}

/// The entity named `name` in `world`.
fn named(world: &GameWorld, name: &str) -> EntityId {
    world
        .iter_entities()
        .find(|entity| {
            world
                .get_component::<Name>(*entity)
                .is_some_and(|known| known.as_str() == name)
        })
        .unwrap_or_else(|| panic!("`{name}` is in the world"))
}

/// **What Play added or removed of declared components, Stop undoes**: a
/// component added while playing is gone, one removed is back with its
/// values — and a `Duration` and an `Angle` come back typed so.
#[test]
fn declared_components_added_or_removed_in_play_are_undone_by_stop() {
    let mut world = GameWorld::new();
    let lantern = declare_lantern(&mut world);
    let fuse = declare_fuse(&mut world);
    let lamp = world.spawn((Transform::identity(), Name::new("Lamp")));
    let bomb = world.spawn((Transform::identity(), Name::new("Bomb")));
    for entity in [lamp, bomb] {
        world
            .inner_world_mut()
            .mark_authored(entity)
            .expect("authored");
    }
    world
        .inner_world_mut()
        .add_runtime_component(
            bomb,
            fuse,
            &ScriptValue::Struct(vec![
                ("wait".to_owned(), ScriptValue::Float(2.5)),
                ("turn".to_owned(), ScriptValue::Float(1.25)),
            ]),
        )
        .expect("Fuse attaches");
    let state = Arc::new(Mutex::new(EditorState::default()));

    apply_play(&mut world, &state);
    {
        let mut state = state.lock().expect("the editor state");
        state.push_edit(PropertyEdit::AddDeclared {
            entity: lamp,
            component: "Lantern".to_owned(),
        });
        state.push_edit(PropertyEdit::RemoveDeclared {
            entity: bomb,
            component: "Fuse".to_owned(),
        });
        crate::ops::apply_edits(&mut world, &mut state);
    }
    assert!(world.inner_world().declared_row(lamp, lantern).is_some());
    assert!(world.inner_world().declared_row(bomb, fuse).is_none());

    apply_stop(&mut world, &state);

    let (lamp, bomb) = (named(&world, "Lamp"), named(&world, "Bomb"));
    assert_eq!(
        world.inner_world().declared_row(lamp, lantern),
        None,
        "the lantern added in Play is gone"
    );
    assert_eq!(
        world.inner_world().declared_row(bomb, fuse),
        Some(vec![
            ("wait".to_owned(), ScriptValue::Float(2.5)),
            ("turn".to_owned(), ScriptValue::Float(1.25)),
        ]),
        "the fuse removed in Play is back"
    );
    let layout = world
        .inner_world()
        .components()
        .vtable(fuse)
        .and_then(|vtable| vtable.columns.packed())
        .expect("Fuse is still declared");
    assert_eq!(layout.ty(0), Some(&ErgonType::Duration));
    assert_eq!(layout.ty(1), Some(&ErgonType::Angle));
}
