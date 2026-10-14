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

//! Components declared at run time, in a scene: saved by name with their
//! fields by name, in every encoding, their entity references remapped, and
//! described by the file itself — a world that never heard of them registers
//! them from it, in the domain the file names.
//!
//! The fixtures here are shared by the other `declared_component_*` files.

use khora_core::math::Vec3;
use khora_core::script::ScriptValue;
use khora_data::ecs::{
    ComponentKey, ComponentProvenance, FieldKind, PackedField, PackedLayout, RuntimeComponentDecl,
    ScriptRole, SemanticDomain, Transform,
};

use super::*;

/// An entity no world holds: the default of an entity field, which has no
/// zero.
pub(super) const NO_ONE: EntityId = EntityId {
    index: u32::MAX,
    generation: u32::MAX,
};

pub(super) fn field(name: &str, kind: FieldKind, default: ScriptValue) -> PackedField {
    PackedField {
        name: name.to_owned(),
        kind,
        default,
    }
}

/// `Health`, as a script declares it: two counters, a label, a target and an
/// optional ally.
pub(super) fn health_fields() -> Vec<PackedField> {
    vec![
        field("current", FieldKind::Int, ScriptValue::Int(100)),
        field("max", FieldKind::Int, ScriptValue::Int(100)),
        field("label", FieldKind::Value, ScriptValue::Str(String::new())),
        field("target", FieldKind::Entity, ScriptValue::Entity(NO_ONE)),
        field("ally", FieldKind::Value, ScriptValue::Null),
    ]
}

/// `Mood`: a counter, a list of words, a nested struct.
pub(super) fn mood_fields() -> Vec<PackedField> {
    vec![
        field("n", FieldKind::Int, ScriptValue::Int(3)),
        field("tags", FieldKind::Value, ScriptValue::Array(Vec::new())),
        field(
            "stash",
            FieldKind::Value,
            ScriptValue::Struct(vec![
                ("item".to_owned(), ScriptValue::Str(String::new())),
                ("value".to_owned(), ScriptValue::Int(0)),
            ]),
        ),
    ]
}

/// Registers the component `name` in `world`, in `domain`, with `fields`.
pub(super) fn declare(
    world: &mut World,
    name: &str,
    domain: SemanticDomain,
    fields: Vec<PackedField>,
) -> ComponentKey {
    world
        .register_runtime_component(RuntimeComponentDecl {
            name: name.to_owned(),
            domain,
            provenance: ComponentProvenance::Authored,
            resumable: false,
            layout: PackedLayout::new(fields).expect("a valid layout"),
        })
        .unwrap_or_else(|e| panic!("`{name}` registers: {e}"))
}

/// The name of `behavior`'s state companion.
pub(super) fn state_of(behavior: &str) -> String {
    format!("{behavior}#state")
}

/// A behavior's state companion's fields — the same for every behavior, with
/// the defaults the scripts declare them with.
pub(super) fn companion_fields() -> Vec<PackedField> {
    vec![
        field("state", FieldKind::Value, ScriptValue::Null),
        field(
            "state_fields",
            FieldKind::Value,
            ScriptValue::Struct(Vec::new()),
        ),
        field("timers", FieldKind::Value, ScriptValue::Array(Vec::new())),
        field("pending", FieldKind::Value, ScriptValue::Null),
        field("spawned", FieldKind::Bool, ScriptValue::Bool(false)),
        field("fault", FieldKind::Value, ScriptValue::Null),
        field(
            "resume_failed",
            FieldKind::Value,
            ScriptValue::Array(Vec::new()),
        ),
    ]
}

/// Registers the behavior `name` that `module` declares, with `fields`, and
/// its state companion, as the scripts' registration registers them: the
/// behavior authored, the companion `Runtime` and resumable, both in
/// `Script`, each with its part for scripts and its declaring module.
/// Returns the behavior's key.
pub(super) fn declare_behavior(
    world: &mut World,
    name: &str,
    module: &str,
    fields: Vec<PackedField>,
) -> ComponentKey {
    fn register(
        world: &mut World,
        name: &str,
        provenance: ComponentProvenance,
        resumable: bool,
        fields: Vec<PackedField>,
    ) -> ComponentKey {
        world
            .register_runtime_component(RuntimeComponentDecl {
                name: name.to_owned(),
                domain: SemanticDomain::Script,
                provenance,
                resumable,
                layout: PackedLayout::new(fields).expect("a valid layout"),
            })
            .unwrap_or_else(|e| panic!("`{name}` registers: {e}"))
    }
    let behavior = register(world, name, ComponentProvenance::Authored, false, fields);
    world.set_script_role(
        behavior,
        Some(ScriptRole::Behavior {
            module: module.to_owned(),
            order: 0,
        }),
    );
    world.set_declared_by(behavior, Some(module.to_owned()));
    let state = register(
        world,
        &state_of(name),
        ComponentProvenance::Runtime,
        true,
        companion_fields(),
    );
    world.set_script_role(
        state,
        Some(ScriptRole::BehaviorState {
            behavior: name.to_owned(),
        }),
    );
    world.set_declared_by(state, Some(module.to_owned()));
    behavior
}

/// A component's value naming its fields: a `Struct` of `pairs`.
pub(super) fn with_fields(pairs: &[(&str, ScriptValue)]) -> ScriptValue {
    ScriptValue::Struct(
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect(),
    )
}

/// The entity `entity`'s `component` field `name` holds.
pub(super) fn entity_field(
    world: &World,
    entity: EntityId,
    component: &str,
    name: &str,
) -> EntityId {
    match field_of(world, entity, component, name) {
        Some(ScriptValue::Entity(target)) => target,
        other => panic!("`{component}.{name}` holds {other:?}, not an entity"),
    }
}

/// Both test components, declared as the scripts declare them.
pub(super) fn declare_both(world: &mut World) {
    declare(world, "Health", SemanticDomain::Physics, health_fields());
    declare(world, "Mood", SemanticDomain::Script, mood_fields());
}

/// Attaches `component` to `entity`: its defaults, then `value`'s fields.
pub(super) fn attach(world: &mut World, entity: EntityId, component: &str, value: ScriptValue) {
    let key = world
        .components()
        .key_named(component)
        .unwrap_or_else(|| panic!("`{component}` is registered"));
    world
        .add_runtime_component(entity, key, &value)
        .unwrap_or_else(|e| panic!("`{component}` attaches: {e:?}"));
}

/// Writes one field of `entity`'s `component`.
pub(super) fn set_field(
    world: &mut World,
    entity: EntityId,
    component: &str,
    name: &str,
    value: ScriptValue,
) {
    let key = world
        .components()
        .key_named(component)
        .unwrap_or_else(|| panic!("`{component}` is registered"));
    let slot = slot_of_field(world, component, name)
        .unwrap_or_else(|| panic!("`{component}` has a field `{name}`"));
    world
        .row_mut(entity, key)
        .unwrap_or_else(|| panic!("the entity holds `{component}`"))
        .set_field(slot, &value)
        .unwrap_or_else(|e| panic!("`{component}.{name}` takes the value: {e:?}"));
}

/// The slot of `component`'s field `name` in `world`'s live declaration.
pub(super) fn slot_of_field(world: &World, component: &str, name: &str) -> Option<usize> {
    let key = world.components().key_named(component)?;
    world
        .components()
        .vtable(key)?
        .columns
        .packed()?
        .slot_of(name)
}

/// The fields `world` declares `component` with, in slot order.
pub(super) fn declared_fields(world: &World, component: &str) -> Option<Vec<PackedField>> {
    let key = world.components().key_named(component)?;
    Some(
        world
            .components()
            .vtable(key)?
            .columns
            .packed()?
            .fields()
            .to_vec(),
    )
}

/// The domain `world` stores `component` in.
pub(super) fn domain_of(world: &World, component: &str) -> Option<SemanticDomain> {
    let key = world.components().key_named(component)?;
    Some(world.components().vtable(key)?.domain)
}

/// The value of `entity`'s `component` field `name`, `None` when the entity
/// holds no such component or the component no such field.
pub(super) fn field_of(
    world: &World,
    entity: EntityId,
    component: &str,
    name: &str,
) -> Option<ScriptValue> {
    let key = world.components().key_named(component)?;
    let slot = slot_of_field(world, component, name)?;
    world.row(entity, key)?.field(slot)
}

/// Every field of `entity`'s `component`, by name, in slot order — `None`
/// when the entity holds none.
pub(super) fn fields_of(
    world: &World,
    entity: EntityId,
    component: &str,
) -> Option<Vec<(String, ScriptValue)>> {
    let key = world.components().key_named(component)?;
    let row = world.row(entity, key)?;
    let fields = declared_fields(world, component)?;
    Some(
        fields
            .iter()
            .enumerate()
            .map(|(slot, field)| {
                (
                    field.name.clone(),
                    row.field(slot).unwrap_or(ScriptValue::Unit),
                )
            })
            .collect(),
    )
}

/// `value` with every entity of `map`'s keys replaced by the one it maps to.
pub(super) fn remapped(value: &ScriptValue, map: &HashMap<EntityId, EntityId>) -> ScriptValue {
    match value {
        ScriptValue::Entity(entity) => ScriptValue::Entity(*map.get(entity).unwrap_or(entity)),
        ScriptValue::Array(items) => {
            ScriptValue::Array(items.iter().map(|item| remapped(item, map)).collect())
        }
        ScriptValue::Struct(fields) => ScriptValue::Struct(
            fields
                .iter()
                .map(|(name, item)| (name.clone(), remapped(item, map)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// A world of four placed entities beside the two declared components:
/// `a` holds both, written and pointing at `c` and `d`; `b` neither; `c` a
/// `Health` of defaults but its target; `d` a `Mood`.
pub(super) struct Declared {
    pub(super) world: World,
    pub(super) a: EntityId,
    pub(super) b: EntityId,
    pub(super) c: EntityId,
    pub(super) d: EntityId,
}

pub(super) fn declared_world() -> Declared {
    let mut world = World::new();
    declare_both(&mut world);
    let placed: Vec<EntityId> = (0..4)
        .map(|i| world.spawn(Transform::from_translation(Vec3::new(i as f32, 1.0, 2.0))))
        .collect();
    let (a, b, c, d) = (placed[0], placed[1], placed[2], placed[3]);
    attach(
        &mut world,
        a,
        "Health",
        ScriptValue::Struct(vec![
            ("current".into(), ScriptValue::Int(40)),
            ("label".into(), ScriptValue::Str("guard".into())),
            ("target".into(), ScriptValue::Entity(c)),
            ("ally".into(), ScriptValue::Entity(d)),
        ]),
    );
    attach(
        &mut world,
        a,
        "Mood",
        ScriptValue::Struct(vec![
            ("n".into(), ScriptValue::Int(7)),
            (
                "tags".into(),
                ScriptValue::Array(vec![
                    ScriptValue::Str("calm".into()),
                    ScriptValue::Str("alert".into()),
                ]),
            ),
            (
                "stash".into(),
                ScriptValue::Struct(vec![
                    ("item".into(), ScriptValue::Str("sword".into())),
                    ("value".into(), ScriptValue::Int(30)),
                ]),
            ),
        ]),
    );
    attach(
        &mut world,
        c,
        "Health",
        ScriptValue::Struct(vec![("target".into(), ScriptValue::Entity(b))]),
    );
    attach(
        &mut world,
        d,
        "Mood",
        ScriptValue::Struct(vec![("n".into(), ScriptValue::Int(-2))]),
    );
    for entity in [a, b, c, d] {
        world.mark_authored(entity).expect("authored");
    }
    Declared { world, a, b, c, d }
}

/// Every translation `world` holds, sorted.
pub(super) fn translations(world: &World) -> Vec<f32> {
    let mut xs: Vec<f32> = world
        .query::<&Transform>()
        .map(|t| t.translation.x)
        .collect();
    xs.sort_by(f32::total_cmp);
    xs
}

/// Asserts every declared component of every entity of `src` is in `dst`,
/// on the entity known by the same identity, field for field, its entity
/// references pointing at the same entities.
pub(super) fn assert_same_declared(src: &World, dst: &World, case: &str) {
    assert_same_rows(src, dst, &["Health", "Mood"], case);
}

/// Asserts every row of each declared component of `components` that an
/// entity of `src` holds is in `dst`, on the entity known by the same
/// identity, field for field, its entity references pointing at the same
/// entities — and that `dst` holds no row `src` did not.
pub(super) fn assert_same_rows(src: &World, dst: &World, components: &[&str], case: &str) {
    let map = entity_map(src, dst);
    for (&entity, &twin) in &map {
        for &component in components {
            let expected = fields_of(src, entity, component).map(|fields| {
                fields
                    .into_iter()
                    .map(|(name, value)| (name, remapped(&value, &map)))
                    .collect::<Vec<_>>()
            });
            assert_eq!(
                fields_of(dst, twin, component),
                expected,
                "{case}: `{component}` of {:?}",
                src.persistent_id(entity)
            );
        }
    }
}

/// A scene holding declared components loads back in every encoding into a
/// world that declares them too and already holds entities: each value as
/// saved, each entity reference pointing at the loaded twin of its target,
/// the Rust components beside them whole, nothing adapted.
#[test]
fn a_scene_round_trips_declared_components_in_every_encoding() {
    let src = declared_world();
    let record = capture_world(&src.world).expect("captures");
    for (name, encoding) in every_encoding() {
        let back = through(&record, name, encoding);
        let mut dst = World::new();
        declare_both(&mut dst);
        // Entities already there, so the loaded ones are not where the
        // saved ones were: a reference copied as is would point elsewhere.
        for _ in 0..3 {
            dst.spawn(Transform::identity());
        }
        let applied = apply(&mut dst, &back, Identity::Keep)
            .unwrap_or_else(|e| panic!("{name}: the load failed: {e}"));
        assert!(
            applied.report.is_clean(),
            "{name}: {:?}",
            applied.report.entries
        );

        let map = entity_map(&src.world, &dst);
        assert!(
            map.iter().any(|(from, to)| from != to),
            "{name}: the loaded entities are not where the saved ones were"
        );
        assert_same_declared(&src.world, &dst, name);
        assert_eq!(
            field_of(&dst, map[&src.a], "Health", "target"),
            Some(ScriptValue::Entity(map[&src.c])),
            "{name}: the target is remapped"
        );
        assert_eq!(
            field_of(&dst, map[&src.a], "Health", "ally"),
            Some(ScriptValue::Entity(map[&src.d])),
            "{name}: a reference inside a value field is remapped"
        );
        assert_eq!(
            field_of(&dst, map[&src.c], "Health", "current"),
            Some(ScriptValue::Int(100)),
            "{name}: a default is saved as a value"
        );
        assert_eq!(fields_of(&dst, map[&src.b], "Health"), None, "{name}");
        assert_eq!(
            translations(&dst),
            vec![0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0],
            "{name}"
        );
    }
}

/// A world that never heard of a declared component — no script ran in it
/// yet, or the project no longer declares it — registers it from the scene
/// that holds it: its name and key, its domain, its fields and defaults.
#[test]
fn a_scene_registers_its_declared_components_in_a_fresh_world() {
    let src = declared_world();
    for (name, encoding) in every_encoding() {
        let (dst, applied) = reload(&src.world, name, encoding);
        assert_eq!(applied.entities.len(), 4, "{name}");
        for (component, domain, fields) in [
            ("Health", SemanticDomain::Physics, health_fields()),
            ("Mood", SemanticDomain::Script, mood_fields()),
        ] {
            assert_eq!(
                dst.components().key_named(component),
                Some(ComponentKey::named(component)),
                "{name}: `{component}` is registered under its name's key"
            );
            assert_eq!(
                domain_of(&dst, component),
                Some(domain),
                "{name}: `{component}` is in the domain the file names"
            );
            let registered = declared_fields(&dst, component)
                .unwrap_or_else(|| panic!("{name}: `{component}` is a declared component"));
            let shape = |fields: &[PackedField]| -> Vec<(String, FieldKind)> {
                fields.iter().map(|f| (f.name.clone(), f.kind)).collect()
            };
            assert_eq!(shape(&registered), shape(&fields), "{name}: `{component}`");
            for (got, want) in registered.iter().zip(&fields) {
                // An entity default names no entity of any world: it is not
                // a value the file can bring back as it was.
                if want.kind != FieldKind::Entity {
                    assert_eq!(
                        got.default, want.default,
                        "{name}: the default of `{component}.{}`",
                        want.name
                    );
                }
            }
        }
        assert_same_declared(&src.world, &dst, name);
        assert_eq!(translations(&dst), vec![0.0, 1.0, 2.0, 3.0], "{name}");
    }
}

/// A component registered from a scene is writable as any declared one: a
/// new row takes the defaults the file described.
#[test]
fn a_component_registered_from_a_scene_attaches_with_its_defaults() {
    let src = declared_world();
    let (mut dst, _) = reload(&src.world, "compact", &CompactEncoding);
    let fresh = dst.spawn(Transform::identity());
    attach(&mut dst, fresh, "Mood", ScriptValue::Unit);
    assert_eq!(
        field_of(&dst, fresh, "Mood", "n"),
        Some(ScriptValue::Int(3))
    );
    assert_eq!(
        field_of(&dst, fresh, "Mood", "tags"),
        Some(ScriptValue::Array(Vec::new()))
    );
}
