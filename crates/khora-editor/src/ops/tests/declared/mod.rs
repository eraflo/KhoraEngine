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

//! The inspector over components a script declared (Ergon spec 11): typed
//! cards read from the World's registry, their edits, "+ Add Component", and
//! what Play shows — through the editor's pure `ops`, no rendering.
//!
//! The world is given its declarations as a mounted project's scripts give
//! them: registered with their layout, the part each plays, and the module
//! that declares it.

mod add_menu;
mod cards;
mod edits;
mod orphans;
mod play;

use khora_sdk::editor_ui::{DeclaredCard, EditorState, InspectedEntity, PlayMode, PropertyEdit};
use khora_sdk::khora_core::script::{ErgonType, ScriptValue};
use khora_sdk::khora_data::ecs::{
    ComponentKey, ComponentProvenance, FieldKind, PackedField, PackedLayout, RuntimeComponentDecl,
    ScriptRole, SemanticDomain,
};
use khora_sdk::prelude::ecs::*;
use khora_sdk::prelude::math::Vec3;
use khora_sdk::GameWorld;

const LANTERN_MODULE: &str = "props/lantern.erg";
const GUARD_MODULE: &str = "ai/guard.erg";
const CLOCK_MODULE: &str = "game/clock.erg";
const COOLDOWN_MODULE: &str = "game/cooldown.erg";

/// What a reference to no entity holds in a test default.
const NOBODY: EntityId = EntityId {
    index: u32::MAX,
    generation: u32::MAX,
};

/// One declaration, as a mounted script registers it.
struct Declaration<'a> {
    name: &'a str,
    module: Option<&'a str>,
    domain: SemanticDomain,
    role: Option<ScriptRole>,
    provenance: ComponentProvenance,
    fields: Vec<(&'a str, ErgonType, ScriptValue)>,
}

/// Registers `declaration` in `world`: its typed layout, the part it plays
/// and the module declaring it — what `apply_declarations` leaves.
fn declare(world: &mut GameWorld, declaration: Declaration<'_>) -> ComponentKey {
    let layout = PackedLayout::typed(
        declaration
            .fields
            .iter()
            .map(|(name, ty, default)| PackedField {
                name: (*name).to_owned(),
                kind: FieldKind::of(ty),
                default: default.clone(),
            })
            .collect(),
        declaration
            .fields
            .iter()
            .map(|(_, ty, _)| ty.clone())
            .collect(),
    )
    .unwrap_or_else(|error| panic!("`{}` has a valid layout: {error:?}", declaration.name));
    let inner = world.inner_world_mut();
    let key = inner
        .register_runtime_component(RuntimeComponentDecl {
            name: declaration.name.to_owned(),
            domain: declaration.domain,
            provenance: declaration.provenance,
            resumable: declaration.provenance == ComponentProvenance::Runtime,
            layout,
        })
        .unwrap_or_else(|error| panic!("`{}` registers: {error:?}", declaration.name));
    inner.set_script_role(key, declaration.role);
    inner.set_declared_by(key, declaration.module.map(str::to_owned));
    key
}

/// `Loot`, a struct a script declares, with its fields.
fn loot_type() -> ErgonType {
    ErgonType::Struct {
        name: "Loot".to_owned(),
        fields: Some(vec![
            ("item".to_owned(), ErgonType::Str),
            ("value".to_owned(), ErgonType::Int),
        ]),
    }
}

/// A `Loot` value.
fn loot(item: &str, value: i64) -> ScriptValue {
    ScriptValue::Struct(vec![
        ("item".to_owned(), ScriptValue::Str(item.to_owned())),
        ("value".to_owned(), ScriptValue::Int(value)),
    ])
}

/// The fields of `Lantern`, in declaration order: one of each kind a card
/// draws.
fn lantern_fields() -> Vec<(&'static str, ErgonType, ScriptValue)> {
    vec![
        ("glow", ErgonType::Float, ScriptValue::Float(1.0)),
        ("count", ErgonType::Int, ScriptValue::Int(3)),
        ("lit", ErgonType::Bool, ScriptValue::Bool(true)),
        ("label", ErgonType::Str, ScriptValue::Str("lamp".to_owned())),
        (
            "offset",
            ErgonType::Engine("Vec3"),
            ScriptValue::Vec3(Vec3::new(0.0, 1.0, 0.0)),
        ),
        ("anchor", ErgonType::Entity, ScriptValue::Entity(NOBODY)),
        (
            "target",
            ErgonType::Optional(Box::new(ErgonType::Entity)),
            ScriptValue::Null,
        ),
        (
            "steps",
            ErgonType::Array(Box::new(ErgonType::Float)),
            ScriptValue::Array(Vec::new()),
        ),
        ("loot", loot_type(), loot("", 0)),
    ]
}

/// `Lantern`, a `component` of `props/lantern.erg`, in `Render`.
fn lantern(world: &mut GameWorld) -> ComponentKey {
    declare(
        world,
        Declaration {
            name: "Lantern",
            module: Some(LANTERN_MODULE),
            domain: SemanticDomain::Render,
            role: None,
            provenance: ComponentProvenance::Authored,
            fields: lantern_fields(),
        },
    )
}

/// The behavior `Guard` of `ai/guard.erg` and its state companion
/// `Guard#state`: the behavior's key, then the companion's.
fn guard(world: &mut GameWorld) -> (ComponentKey, ComponentKey) {
    let behavior = declare(
        world,
        Declaration {
            name: "Guard",
            module: Some(GUARD_MODULE),
            domain: SemanticDomain::Script,
            role: Some(ScriptRole::Behavior {
                module: GUARD_MODULE.to_owned(),
                order: 0,
            }),
            provenance: ComponentProvenance::Authored,
            fields: vec![("speed", ErgonType::Float, ScriptValue::Float(1.5))],
        },
    );
    let companion = declare(
        world,
        Declaration {
            name: "Guard#state",
            module: Some(GUARD_MODULE),
            domain: SemanticDomain::Script,
            role: Some(ScriptRole::BehaviorState {
                behavior: "Guard".to_owned(),
            }),
            provenance: ComponentProvenance::Runtime,
            fields: vec![
                (
                    "state",
                    ErgonType::Optional(Box::new(ErgonType::Str)),
                    ScriptValue::Null,
                ),
                ("timers", ErgonType::Any, ScriptValue::Array(Vec::new())),
                ("fault", ErgonType::Any, ScriptValue::Null),
            ],
        },
    );
    (behavior, companion)
}

/// The system `Clock` of `game/clock.erg`.
fn clock(world: &mut GameWorld) -> ComponentKey {
    declare(
        world,
        Declaration {
            name: "Clock",
            module: Some(CLOCK_MODULE),
            domain: SemanticDomain::Script,
            role: Some(ScriptRole::System {
                module: CLOCK_MODULE.to_owned(),
                order: 0,
            }),
            provenance: ComponentProvenance::Runtime,
            fields: vec![("ticks", ErgonType::Int, ScriptValue::Int(0))],
        },
    )
}

/// `Relic`, a component no module declares any more: an orphan kept with
/// its data.
fn relic(world: &mut GameWorld) -> ComponentKey {
    declare(
        world,
        Declaration {
            name: "Relic",
            module: None,
            domain: SemanticDomain::Script,
            role: None,
            provenance: ComponentProvenance::Authored,
            fields: vec![("power", ErgonType::Int, ScriptValue::Int(7))],
        },
    )
}

/// `Cooldown`, whose fields are a `Duration` and an `Angle`.
fn cooldown(world: &mut GameWorld) -> ComponentKey {
    declare(
        world,
        Declaration {
            name: "Cooldown",
            module: Some(COOLDOWN_MODULE),
            domain: SemanticDomain::Script,
            role: None,
            provenance: ComponentProvenance::Authored,
            fields: vec![
                ("wait", ErgonType::Duration, ScriptValue::Float(2.0)),
                ("turn", ErgonType::Angle, ScriptValue::Float(90.0)),
            ],
        },
    )
}

/// Gives `entity` the declared component `key` at its defaults.
fn attach(world: &mut GameWorld, entity: EntityId, key: ComponentKey) {
    world
        .inner_world_mut()
        .add_runtime_component(entity, key, &ScriptValue::Unit)
        .unwrap_or_else(|error| panic!("{key:?} attaches: {error:?}"));
}

/// Writes `entity`'s `field` of `key` directly, as a script would.
fn set(
    world: &mut GameWorld,
    entity: EntityId,
    key: ComponentKey,
    field: &str,
    value: ScriptValue,
) {
    let inner = world.inner_world_mut();
    let slot = inner
        .components()
        .vtable(key)
        .and_then(|vtable| vtable.columns.packed())
        .and_then(|layout| layout.slot_of(field))
        .unwrap_or_else(|| panic!("{key:?} has `{field}`"));
    inner
        .row_mut(entity, key)
        .expect("the entity holds the component")
        .write_fields(&[(slot, &value)])
        .unwrap_or_else(|error| panic!("`{field}` takes {value:?}: {error:?}"));
}

/// `entity`'s row of the declared component `name`, its fields by name.
fn row(world: &GameWorld, entity: EntityId, name: &str) -> Option<Vec<(String, ScriptValue)>> {
    let inner = world.inner_world();
    let key = inner.components().key_named(name)?;
    inner.declared_row(entity, key)
}

/// The value of `entity`'s `field` of `name`.
fn field_of(world: &GameWorld, entity: EntityId, name: &str, field: &str) -> Option<ScriptValue> {
    row(world, entity, name)?
        .into_iter()
        .find(|(known, _)| known == field)
        .map(|(_, value)| value)
}

/// The inspected view of `entity` while the editor is in `mode`, as a
/// frame's `extract_inspected` leaves it.
fn inspect(world: &GameWorld, entity: EntityId, mode: PlayMode) -> InspectedEntity {
    let mut state = EditorState {
        play_mode: mode,
        ..EditorState::default()
    };
    state.selection.insert(entity);
    crate::ops::extract_inspected(world, &mut state);
    state.inspected.expect("the selected entity is inspected")
}

/// The card of `name` in `inspected`, if it shows one.
fn card<'i>(inspected: &'i InspectedEntity, name: &str) -> Option<&'i DeclaredCard> {
    inspected.declared.iter().find(|card| card.name == name)
}

/// The card of `name` in `inspected`, which must show one.
fn card_of<'i>(inspected: &'i InspectedEntity, name: &str) -> &'i DeclaredCard {
    card(inspected, name).unwrap_or_else(|| {
        panic!(
            "a `{name}` card; the cards are {:?}",
            inspected
                .declared
                .iter()
                .map(|card| &card.name)
                .collect::<Vec<_>>()
        )
    })
}

/// Applies `edits` as the inspector's next frame does.
fn apply(world: &mut GameWorld, edits: Vec<PropertyEdit>) {
    let mut state = EditorState::default();
    for edit in edits {
        state.push_edit(edit);
    }
    crate::ops::apply_edits(world, &mut state);
}

/// The edit setting `entity`'s `field` of `component` to `value`.
fn set_field(entity: EntityId, component: &str, field: &str, value: ScriptValue) -> PropertyEdit {
    PropertyEdit::SetDeclaredField {
        entity,
        component: component.to_owned(),
        field: field.to_owned(),
        value,
    }
}
