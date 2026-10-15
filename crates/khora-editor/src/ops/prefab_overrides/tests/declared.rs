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

//! An instance's declared components against its prefab's, field by field:
//! an overridden field is marked, an added or removed component too, and
//! each is reverted with the declared-component edits — the prefab's values
//! being what the inspected view carries.

use khora_sdk::khora_core::script::{ErgonType, ScriptValue};
use khora_sdk::khora_data::ecs::{
    ComponentKey, ComponentProvenance, FieldKind, PackedField, PackedLayout, RuntimeComponentDecl,
    SemanticDomain,
};

use super::*;

const MODULE: &str = "props/lantern.erg";

/// Registers `Lantern { float glow = 1.0; string label = "lamp"; }`, as
/// `props/lantern.erg` declares it.
fn declare_lantern(world: &mut World) -> ComponentKey {
    let fields = [
        ("glow", ErgonType::Float, ScriptValue::Float(1.0)),
        ("label", ErgonType::Str, ScriptValue::Str("lamp".to_owned())),
    ];
    let key = world
        .register_runtime_component(RuntimeComponentDecl {
            name: "Lantern".to_owned(),
            domain: SemanticDomain::Render,
            provenance: ComponentProvenance::Authored,
            resumable: false,
            layout: PackedLayout::typed(
                fields
                    .iter()
                    .map(|(name, ty, default)| PackedField {
                        name: (*name).to_owned(),
                        kind: FieldKind::of(ty),
                        default: default.clone(),
                    })
                    .collect(),
                fields.iter().map(|(_, ty, _)| ty.clone()).collect(),
            )
            .expect("a valid layout"),
        })
        .expect("Lantern registers");
    world.set_declared_by(key, Some(MODULE.to_owned()));
    key
}

/// The prefab's `Lantern`: `glow` 2, `label` "post".
fn prefab_lantern() -> Vec<(String, ScriptValue)> {
    vec![
        ("glow".to_owned(), ScriptValue::Float(2.0)),
        ("label".to_owned(), ScriptValue::Str("post".to_owned())),
    ]
}

/// A lamp-post instance placed in an editor world whose scripts declare
/// `Lantern`: a root lit by the prefab's `Lantern`, and a wick with none.
struct Post {
    world: GameWorld,
    library: Library,
    prefab: AssetUUID,
    root: EntityId,
    wick: EntityId,
    key: ComponentKey,
}

fn post() -> Post {
    let mut made = World::new();
    let key = declare_lantern(&mut made);
    let root = made.spawn((Transform::identity(), Name::new("Post")));
    let wick = made.spawn((Transform::identity(), Name::new("Wick")));
    made.set_parent(wick, Some(root));
    made.add_runtime_component(root, key, &ScriptValue::Struct(prefab_lantern()))
        .expect("the post's lantern attaches");
    made.mark_authored(root).expect("authored");
    let wick_id = made.mark_authored(wick).expect("authored");
    let prefab = AssetUUID::new();
    let library = Library(HashMap::from([(
        prefab,
        capture_subtree(&made, root).expect("the post captures"),
    )]));

    let mut world = GameWorld::new();
    let key = declare_lantern(world.inner_world_mut());
    let root = instantiate_prefab(world.inner_world_mut(), prefab, &library)
        .expect("the post instantiates");
    let instance = world
        .inner_world()
        .persistent_id(root)
        .expect("the root has an id");
    let wick = world
        .inner_world()
        .entity_with_id(PersistentId::within(instance, wick_id))
        .expect("the wick is there");
    Post {
        world,
        library,
        prefab,
        root,
        wick,
        key,
    }
}

impl Post {
    fn inspect(&self, entity: EntityId) -> InspectedPrefab {
        inspect_prefab(&self.world, entity, &self.library)
            .unwrap_or_else(|| panic!("{entity:?} belongs to no instance"))
    }

    /// Writes `entity`'s `field` of `Lantern` as a script would.
    fn set(&mut self, entity: EntityId, field: &str, value: ScriptValue) {
        let inner = self.world.inner_world_mut();
        let slot = inner
            .components()
            .vtable(self.key)
            .and_then(|vtable| vtable.columns.packed())
            .and_then(|layout| layout.slot_of(field))
            .unwrap_or_else(|| panic!("Lantern has `{field}`"));
        inner
            .row_mut(entity, self.key)
            .expect("the entity holds Lantern")
            .write_fields(&[(slot, &value)])
            .expect("the field takes the value");
    }

    fn lantern(&self, entity: EntityId) -> Option<Vec<(String, ScriptValue)>> {
        self.world.inner_world().declared_row(entity, self.key)
    }
}

/// The prefab's fields of the declared component `name` in `inspected`.
fn prefab_fields<'a>(
    inspected: &'a InspectedPrefab,
    name: &str,
) -> Option<&'a Vec<(String, ScriptValue)>> {
    inspected
        .prefab_declared
        .iter()
        .find(|(component, _)| component == name)
        .map(|(_, fields)| fields)
}

/// **An instance's declared field set apart from its prefab's is marked**,
/// by its field name — and an untouched instance marks nothing.
#[test]
fn an_overridden_declared_field_is_marked() {
    let mut post = post();
    assert_eq!(post.inspect(post.root).override_count(), 0, "untouched");
    post.set(post.root, "glow", ScriptValue::Float(5.0));

    let inspected = post.inspect(post.root);

    assert!(
        inspected.components.contains(&ComponentOverride {
            type_name: "Lantern".to_owned(),
            added: false,
            fields: vec![path(&["glow"])],
        }),
        "{:?}",
        inspected.components
    );
    assert_eq!(inspected.fields_of("Lantern"), &[path(&["glow"])][..]);
    assert_eq!(inspected.override_count(), 1);
    assert_eq!(
        prefab_fields(&inspected, "Lantern"),
        Some(&prefab_lantern()),
        "the prefab's values, for a revert"
    );
}

/// **A declared component the instance added is marked added.**
#[test]
fn a_declared_component_the_instance_added_is_marked_added() {
    let mut post = post();
    post.world
        .inner_world_mut()
        .add_runtime_component(post.wick, post.key, &ScriptValue::Unit)
        .expect("a lantern attaches to the wick");

    let inspected = post.inspect(post.wick);

    assert!(
        inspected.components.contains(&ComponentOverride {
            type_name: "Lantern".to_owned(),
            added: true,
            fields: Vec::new(),
        }),
        "{:?}",
        inspected.components
    );
    assert_eq!(inspected.override_count(), 1);
}

/// **A declared component the instance removed is marked removed**, with
/// the prefab's values to bring it back.
#[test]
fn a_declared_component_the_instance_removed_is_marked_removed() {
    let mut post = post();
    assert!(post
        .world
        .inner_world_mut()
        .remove_component_by_key(post.root, post.key));

    let inspected = post.inspect(post.root);

    assert_eq!(inspected.removed, vec!["Lantern".to_owned()]);
    assert_eq!(
        prefab_fields(&inspected, "Lantern"),
        Some(&prefab_lantern())
    );
}

/// **Reverting a declared field** — the prefab's value set back with a
/// declared-field edit — clears its override and keeps the others.
#[test]
fn reverting_a_declared_field_clears_its_override_and_keeps_the_others() {
    let mut post = post();
    post.set(post.root, "glow", ScriptValue::Float(5.0));
    post.set(post.root, "label", ScriptValue::Str("mine".to_owned()));
    let inspected = post.inspect(post.root);
    let glow = prefab_fields(&inspected, "Lantern")
        .and_then(|fields| fields.iter().find(|(name, _)| name == "glow"))
        .map(|(_, value)| value.clone())
        .expect("the prefab's glow");

    apply(
        &mut post.world,
        vec![PropertyEdit::SetDeclaredField {
            entity: post.root,
            component: "Lantern".to_owned(),
            field: "glow".to_owned(),
            value: glow,
        }],
    );

    assert_eq!(
        post.lantern(post.root),
        Some(vec![
            ("glow".to_owned(), ScriptValue::Float(2.0)),
            ("label".to_owned(), ScriptValue::Str("mine".to_owned())),
        ])
    );
    assert_eq!(
        post.inspect(post.root).fields_of("Lantern"),
        &[path(&["label"])][..]
    );
}

/// **Reverting a removed declared component brings back the prefab's** —
/// added again, each field set to the prefab's value: the root overrides
/// nothing any more.
#[test]
fn reverting_a_removed_declared_component_brings_back_the_prefabs() {
    let mut post = post();
    post.world
        .inner_world_mut()
        .remove_component_by_key(post.root, post.key);
    let inspected = post.inspect(post.root);
    let fields = prefab_fields(&inspected, "Lantern")
        .expect("the prefab's lantern")
        .clone();
    let mut edits = vec![PropertyEdit::AddDeclared {
        entity: post.root,
        component: "Lantern".to_owned(),
    }];
    edits.extend(
        fields
            .into_iter()
            .map(|(field, value)| PropertyEdit::SetDeclaredField {
                entity: post.root,
                component: "Lantern".to_owned(),
                field,
                value,
            }),
    );

    apply(&mut post.world, edits);

    assert_eq!(post.lantern(post.root), Some(prefab_lantern()));
    let inspected = post.inspect(post.root);
    assert!(inspected.removed.is_empty(), "{:?}", inspected.removed);
    assert_eq!(inspected.override_count(), 0);
}

/// **Reverting an added declared component removes it.**
#[test]
fn reverting_an_added_declared_component_removes_it() {
    let mut post = post();
    post.world
        .inner_world_mut()
        .add_runtime_component(post.wick, post.key, &ScriptValue::Unit)
        .expect("a lantern attaches to the wick");
    assert_eq!(post.inspect(post.wick).override_count(), 1, "added");

    apply(
        &mut post.world,
        vec![PropertyEdit::RemoveDeclared {
            entity: post.wick,
            component: "Lantern".to_owned(),
        }],
    );

    assert_eq!(post.inspect(post.wick).override_count(), 0);
}

/// **Reverting the instance takes its declared components back too**: a
/// field set apart, a component added.
#[test]
fn reverting_the_instance_reverts_its_declared_components() {
    let mut post = post();
    post.set(post.root, "glow", ScriptValue::Float(5.0));
    post.world
        .inner_world_mut()
        .add_runtime_component(post.wick, post.key, &ScriptValue::Unit)
        .expect("a lantern attaches to the wick");
    let instance = InstanceOf {
        root: post.root,
        prefab: post.prefab,
    };
    let scratch = prefab_world(post.world.inner_world(), &instance, &post.library)
        .expect("the prefab expands");

    let edits = revert_instance(post.world.inner_world(), &instance, &scratch);
    apply(&mut post.world, edits);

    assert_eq!(post.lantern(post.root), Some(prefab_lantern()));
    assert_eq!(post.lantern(post.wick), None, "the added lantern is gone");
    for member in [post.root, post.wick] {
        assert_eq!(post.inspect(member).override_count(), 0, "{member:?}");
    }
}

/// Registers the declared component `name` of `fields` in `world`, as
/// `MODULE` declares it.
fn declare_fields(
    world: &mut World,
    name: &str,
    fields: &[(&str, ErgonType, ScriptValue)],
) -> ComponentKey {
    let key = world
        .register_runtime_component(RuntimeComponentDecl {
            name: name.to_owned(),
            domain: SemanticDomain::Script,
            provenance: ComponentProvenance::Authored,
            resumable: false,
            layout: PackedLayout::typed(
                fields
                    .iter()
                    .map(|(field, ty, default)| PackedField {
                        name: (*field).to_owned(),
                        kind: FieldKind::of(ty),
                        default: default.clone(),
                    })
                    .collect(),
                fields.iter().map(|(_, ty, _)| ty.clone()).collect(),
            )
            .expect("a valid layout"),
        })
        .unwrap_or_else(|error| panic!("`{name}` registers: {error:?}"));
    world.set_declared_by(key, Some(MODULE.to_owned()));
    key
}

/// A one-entity prefab whose root holds `Lantern` at `values` — saved while
/// the script declared `saved` — placed in an editor world whose script
/// declares `live` now: the world, the prefab library and the root.
fn saved_then_redeclared(
    saved: &[(&str, ErgonType, ScriptValue)],
    values: Vec<(String, ScriptValue)>,
    live: &[(&str, ErgonType, ScriptValue)],
) -> (GameWorld, Library, EntityId) {
    let mut made = World::new();
    let key = declare_fields(&mut made, "Lantern", saved);
    let root = made.spawn((Transform::identity(), Name::new("Post")));
    made.add_runtime_component(root, key, &ScriptValue::Struct(values))
        .expect("the post's lantern attaches");
    made.mark_authored(root).expect("authored");
    let prefab = AssetUUID::new();
    let library = Library(HashMap::from([(
        prefab,
        capture_subtree(&made, root).expect("the post captures"),
    )]));

    let mut world = GameWorld::new();
    declare_fields(world.inner_world_mut(), "Lantern", live);
    let root = instantiate_prefab(world.inner_world_mut(), prefab, &library)
        .expect("the post instantiates");
    (world, library, root)
}

/// **A field the script declared after the prefab was saved is no
/// override**: the prefab gives it no value, so an instance that never
/// touched it holds the declaration's default — as the prefab itself would.
#[test]
fn a_field_declared_after_the_prefab_was_saved_is_no_override() {
    let (world, library, root) = saved_then_redeclared(
        &[("glow", ErgonType::Float, ScriptValue::Float(1.0))],
        vec![("glow".to_owned(), ScriptValue::Float(2.0))],
        &[
            ("glow", ErgonType::Float, ScriptValue::Float(1.0)),
            ("label", ErgonType::Str, ScriptValue::Str("lamp".to_owned())),
        ],
    );

    let inspected = inspect_prefab(&world, root, &library).expect("the post is an instance");

    assert_eq!(
        inspected.override_count(),
        0,
        "an untouched instance overrides nothing: {:?}",
        inspected.components
    );
}

/// **A field widened from `int` to `float` since the prefab was saved is no
/// override**: the instance loaded the prefab's 2 as 2.0 — the prefab's own
/// value, read under the declaration the world has now.
#[test]
fn a_field_widened_since_the_prefab_was_saved_is_no_override() {
    let (world, library, root) = saved_then_redeclared(
        &[("glow", ErgonType::Int, ScriptValue::Int(1))],
        vec![("glow".to_owned(), ScriptValue::Int(2))],
        &[("glow", ErgonType::Float, ScriptValue::Float(1.0))],
    );
    let key = world
        .inner_world()
        .components()
        .key_named("Lantern")
        .expect("Lantern is declared");
    assert_eq!(
        world.inner_world().declared_row(root, key),
        Some(vec![("glow".to_owned(), ScriptValue::Float(2.0))]),
        "the instance holds the prefab's glow, widened"
    );

    let inspected = inspect_prefab(&world, root, &library).expect("the post is an instance");

    assert_eq!(
        inspected.override_count(),
        0,
        "an untouched instance overrides nothing: {:?}",
        inspected.components
    );
}

/// A two-member prefab — a post whose `Beacon` points at its own wick —
/// placed in an editor world: the world, the library, the prefab, the
/// placed root and wick.
fn beacon_post() -> (GameWorld, Library, AssetUUID, EntityId, EntityId) {
    let fields = [(
        "target",
        ErgonType::Optional(Box::new(ErgonType::Entity)),
        ScriptValue::Null,
    )];
    let mut made = World::new();
    let key = declare_fields(&mut made, "Beacon", &fields);
    let root = made.spawn((Transform::identity(), Name::new("Post")));
    let wick = made.spawn((Transform::identity(), Name::new("Wick")));
    made.set_parent(wick, Some(root));
    made.add_runtime_component(
        root,
        key,
        &ScriptValue::Struct(vec![("target".to_owned(), ScriptValue::Entity(wick))]),
    )
    .expect("the post's beacon attaches");
    made.mark_authored(root).expect("authored");
    let wick_id = made.mark_authored(wick).expect("authored");
    let prefab = AssetUUID::new();
    let library = Library(HashMap::from([(
        prefab,
        capture_subtree(&made, root).expect("the post captures"),
    )]));

    let mut world = GameWorld::new();
    declare_fields(world.inner_world_mut(), "Beacon", &fields);
    let root = instantiate_prefab(world.inner_world_mut(), prefab, &library)
        .expect("the post instantiates");
    let instance = world
        .inner_world()
        .persistent_id(root)
        .expect("the root has an id");
    let wick = world
        .inner_world()
        .entity_with_id(PersistentId::within(instance, wick_id))
        .expect("the wick is there");
    (world, library, prefab, root, wick)
}

/// The `target` of `entity`'s `Beacon`.
fn beacon_target(world: &GameWorld, entity: EntityId) -> Option<ScriptValue> {
    let key = world.inner_world().components().key_named("Beacon")?;
    world
        .inner_world()
        .declared_row(entity, key)?
        .into_iter()
        .find(|(name, _)| name == "target")
        .map(|(_, value)| value)
}

/// **An entity field naming a member of the instance is no override**: the
/// prefab's reference to its wick is the instance's reference to its own.
#[test]
fn an_entity_field_naming_a_member_is_no_override_on_an_untouched_instance() {
    let (world, library, _, root, wick) = beacon_post();
    assert_eq!(
        beacon_target(&world, root),
        Some(ScriptValue::Entity(wick)),
        "the placed post points at its own wick"
    );

    let inspected = inspect_prefab(&world, root, &library).expect("the post is an instance");

    assert_eq!(
        inspected.override_count(),
        0,
        "untouched: {:?}",
        inspected.components
    );
    assert_eq!(
        prefab_fields(&inspected, "Beacon"),
        Some(&vec![("target".to_owned(), ScriptValue::Entity(wick))]),
        "the prefab's value names the instance's wick, ready for a revert"
    );
}

/// **Reverting an entity field points it at the instance's own member** —
/// not at the prefab expansion's entity, nor at nothing.
#[test]
fn reverting_an_entity_field_points_it_at_the_instances_own_member() {
    let (mut world, library, prefab, root, wick) = beacon_post();
    let key = world
        .inner_world()
        .components()
        .key_named("Beacon")
        .expect("Beacon is declared");
    world
        .inner_world_mut()
        .row_mut(root, key)
        .expect("the root holds Beacon")
        .write_fields(&[(0, &ScriptValue::Null)])
        .expect("target takes none");
    assert_eq!(
        inspect_prefab(&world, root, &library)
            .expect("the post is an instance")
            .fields_of("Beacon"),
        &[path(&["target"])][..]
    );
    let instance = InstanceOf { root, prefab };
    let scratch =
        prefab_world(world.inner_world(), &instance, &library).expect("the prefab expands");

    let edits = revert_instance(world.inner_world(), &instance, &scratch);
    apply(&mut world, edits);

    assert_eq!(beacon_target(&world, root), Some(ScriptValue::Entity(wick)));
    assert_eq!(
        inspect_prefab(&world, root, &library)
            .expect("the post is an instance")
            .override_count(),
        0
    );
}

/// **An `int` too large for a `float` since the prefab was saved is no
/// override**: a load does not round it — the instance holds the field's
/// default — so the prefab's value, read the way a load reads it, is that
/// default too.
#[test]
fn an_int_past_what_a_float_holds_since_the_prefab_was_saved_is_no_override() {
    let (world, library, root) = saved_then_redeclared(
        &[("glow", ErgonType::Int, ScriptValue::Int(1))],
        vec![("glow".to_owned(), ScriptValue::Int(16_777_217))],
        &[("glow", ErgonType::Float, ScriptValue::Float(1.0))],
    );
    let key = world
        .inner_world()
        .components()
        .key_named("Lantern")
        .expect("Lantern is declared");
    assert_eq!(
        world.inner_world().declared_row(root, key),
        Some(vec![("glow".to_owned(), ScriptValue::Float(1.0))]),
        "the load did not round 2^24 + 1: the glow is its default"
    );

    let inspected = inspect_prefab(&world, root, &library).expect("the post is an instance");

    assert_eq!(
        inspected.override_count(),
        0,
        "an untouched instance overrides nothing: {:?}",
        inspected.components
    );
}

/// **A field whose type changed beyond widening since the prefab was saved
/// is no override**: neither the instance nor the prefab, read under the
/// new declaration, keeps the old value — both hold the default.
#[test]
fn a_field_retyped_since_the_prefab_was_saved_is_no_override() {
    let (world, library, root) = saved_then_redeclared(
        &[("label", ErgonType::Str, ScriptValue::Str("lamp".to_owned()))],
        vec![("label".to_owned(), ScriptValue::Str("post".to_owned()))],
        &[("label", ErgonType::Int, ScriptValue::Int(0))],
    );

    let inspected = inspect_prefab(&world, root, &library).expect("the post is an instance");

    assert_eq!(
        inspected.override_count(),
        0,
        "an untouched instance overrides nothing: {:?}",
        inspected.components
    );
}

/// **A prefab's declared component no script declares any more is no
/// override** on an untouched instance: the instance keeps it, as the file
/// declares it, and so does the prefab.
#[test]
fn a_declared_component_no_script_declares_is_no_override_on_an_untouched_instance() {
    let mut made = World::new();
    let key = declare_fields(
        &mut made,
        "Lantern",
        &[("glow", ErgonType::Float, ScriptValue::Float(1.0))],
    );
    let root = made.spawn((Transform::identity(), Name::new("Post")));
    made.add_runtime_component(
        root,
        key,
        &ScriptValue::Struct(vec![("glow".to_owned(), ScriptValue::Float(2.0))]),
    )
    .expect("the post's lantern attaches");
    made.mark_authored(root).expect("authored");
    let prefab = AssetUUID::new();
    let library = Library(HashMap::from([(
        prefab,
        capture_subtree(&made, root).expect("the post captures"),
    )]));
    let mut world = GameWorld::new();
    let root = instantiate_prefab(world.inner_world_mut(), prefab, &library)
        .expect("the post instantiates");

    let inspected = inspect_prefab(&world, root, &library).expect("the post is an instance");

    assert_eq!(
        inspected.override_count(),
        0,
        "an untouched instance overrides nothing: {:?}",
        inspected.components
    );
}
