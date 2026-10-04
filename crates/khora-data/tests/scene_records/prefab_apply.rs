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

//! An instance's overrides written back into its prefab: a field, a
//! component or the whole instance — and the prefab that comes out is one
//! every other instance follows.

use khora_core::asset::AssetUUID;
use khora_core::math::Vec3;
use khora_data::ecs::{Light, Name, Tag, Transform};
use khora_data::scene::{
    apply_to_prefab, instance_of, instantiate_prefab, InstanceOf, PrefabApply,
};

use super::prefab_sample::*;
use super::*;

/// A turret instance placed in a world of its own.
pub(super) struct Placed {
    pub(super) world: World,
    pub(super) library: Library,
    pub(super) prefab: AssetUUID,
    pub(super) turret: Turret,
    /// The instance root, in `world`.
    pub(super) root: EntityId,
    /// The instance root's identity.
    pub(super) instance: PersistentId,
}

impl Placed {
    /// The instance's member known in the prefab as `inner`.
    pub(super) fn member(&self, inner: PersistentId) -> EntityId {
        entity(&self.world, PersistentId::within(self.instance, inner))
    }

    pub(super) fn of(&self) -> InstanceOf {
        InstanceOf {
            root: self.root,
            prefab: self.prefab,
        }
    }

    /// `what` applied to the turret prefab.
    pub(super) fn apply(&self, what: PrefabApply) -> SceneRecord {
        apply_to_prefab(&self.world, &self.of(), &what, &self.library)
            .unwrap_or_else(|e| panic!("the apply is refused: {e}"))
    }
}

pub(super) fn placed() -> Placed {
    let turret = turret();
    let prefab = AssetUUID::new();
    let library = Library::default().with(prefab, turret.record.clone());
    let mut world = World::new();
    let root = instantiate_prefab(&mut world, prefab, &library).expect("the turret instantiates");
    let instance = id(&world, root);
    Placed {
        world,
        library,
        prefab,
        turret,
        root,
        instance,
    }
}

fn transform_mut(world: &mut World, entity: EntityId) -> &mut Transform {
    world.get_mut::<Transform>(entity).expect("placed")
}

fn field(entity: EntityId, component: &str, path: &[&str]) -> PrefabApply {
    PrefabApply::Field {
        entity,
        component: component.to_owned(),
        path: path.iter().map(|step| (*step).to_owned()).collect(),
    }
}

pub(super) fn component(entity: EntityId, component: &str) -> PrefabApply {
    PrefabApply::Component {
        entity,
        component: component.to_owned(),
    }
}

fn scale(world: &World, entity: EntityId) -> Vec3 {
    world.get::<Transform>(entity).expect("placed").scale
}

fn tagged(world: &World, entity: EntityId) -> bool {
    world
        .get::<Tag>(entity)
        .is_some_and(|tag| tag.contains("armed"))
}

/// Whether any page of `record` holds a `component` column.
fn holds_component(record: &SceneRecord, component: &str) -> bool {
    record
        .pages
        .iter()
        .any(|page| page.components.iter().any(|name| name == component))
}

/// Applying the barrel's translation writes that field alone into the
/// prefab, in the prefab's own ids: the scale and the name the scene also
/// changed on the barrel stay as the prefab had them — and stay overrides of
/// the instance once it is saved against the new prefab and opened again.
#[test]
fn applying_a_field_writes_only_that_field_into_the_prefab() {
    let mut placed = placed();
    let barrel = placed.member(placed.turret.barrel);
    transform_mut(&mut placed.world, barrel).translation.x = 5.0;
    transform_mut(&mut placed.world, barrel).scale = Vec3::new(2.0, 2.0, 2.0);
    placed.world.get_mut::<Name>(barrel).expect("named").0 = "Long barrel".into();

    let written = placed.apply(field(barrel, "Transform", &["translation"]));

    assert_eq!(
        written.entities[0], placed.turret.root,
        "the prefab keeps its own root id"
    );
    assert!(written.instances.is_empty(), "the turret holds no instance");
    let prefab = open(&written, &placed.library);
    let prefab_barrel = entity(&prefab, placed.turret.barrel);
    assert_eq!(
        translation(&prefab, prefab_barrel),
        Vec3::new(5.0, 1.0, 0.0)
    );
    assert_eq!(scale(&prefab, prefab_barrel), Vec3::ONE, "the scale stays");
    assert_eq!(name(&prefab, prefab_barrel), "Barrel", "the name stays");

    let library = placed.library.clone().with(placed.prefab, written);
    let loaded = open(&save(&placed.world, &library), &library);
    let barrel = entity(
        &loaded,
        PersistentId::within(placed.instance, placed.turret.barrel),
    );
    assert_eq!(translation(&loaded, barrel), Vec3::new(5.0, 1.0, 0.0));
    assert_eq!(
        scale(&loaded, barrel),
        Vec3::new(2.0, 2.0, 2.0),
        "the scale is still the instance's override"
    );
    assert_eq!(name(&loaded, barrel), "Long barrel");
}

/// A path that reaches one leaf of a field writes that leaf alone: the
/// barrel pushed sideways and raised gives the prefab the push only, and the
/// raise stays the instance's.
#[test]
fn applying_a_leaf_writes_that_leaf_alone() {
    let mut placed = placed();
    let barrel = placed.member(placed.turret.barrel);
    transform_mut(&mut placed.world, barrel).translation = Vec3::new(5.0, 7.0, 0.0);

    let written = placed.apply(field(barrel, "Transform", &["translation", "x"]));

    let prefab = open(&written, &placed.library);
    assert_eq!(
        translation(&prefab, entity(&prefab, placed.turret.barrel)),
        Vec3::new(5.0, 1.0, 0.0),
        "the prefab takes x, keeps its y"
    );
    let library = placed.library.clone().with(placed.prefab, written);
    let loaded = open(&save(&placed.world, &library), &library);
    let barrel = entity(
        &loaded,
        PersistentId::within(placed.instance, placed.turret.barrel),
    );
    assert_eq!(translation(&loaded, barrel), Vec3::new(5.0, 7.0, 0.0));
}

/// Applying a component writes it whole — every field the instance changed
/// on it — and nothing of another component or another member.
#[test]
fn applying_a_component_writes_it_whole_and_nothing_else() {
    let mut placed = placed();
    let barrel = placed.member(placed.turret.barrel);
    transform_mut(&mut placed.world, barrel).translation.x = 5.0;
    transform_mut(&mut placed.world, barrel).scale = Vec3::new(2.0, 2.0, 2.0);
    placed.world.get_mut::<Name>(barrel).expect("named").0 = "Long barrel".into();
    placed.world.get_mut::<Name>(placed.root).expect("named").0 = "Gate turret".into();

    let written = placed.apply(component(barrel, "Transform"));

    let prefab = open(&written, &placed.library);
    let prefab_barrel = entity(&prefab, placed.turret.barrel);
    assert_eq!(
        translation(&prefab, prefab_barrel),
        Vec3::new(5.0, 1.0, 0.0)
    );
    assert_eq!(scale(&prefab, prefab_barrel), Vec3::new(2.0, 2.0, 2.0));
    assert_eq!(name(&prefab, prefab_barrel), "Barrel", "another component");
    assert_eq!(
        name(&prefab, entity(&prefab, placed.turret.root)),
        "Turret",
        "another member"
    );
}

/// A component the scene added to a member reaches the prefab when it is
/// applied: the sight's tag is the prefab's from then on, and no other member
/// gains it.
#[test]
fn applying_a_component_added_to_a_member_adds_it_to_the_prefab() {
    let mut placed = placed();
    let sight = placed.member(placed.turret.sight);
    placed
        .world
        .add_component(sight, Tag::from_iter(["armed"]))
        .expect("a tag attaches");

    let written = placed.apply(component(sight, "Tag"));

    let prefab = open(&written, &placed.library);
    assert!(tagged(&prefab, entity(&prefab, placed.turret.sight)));
    assert!(
        prefab
            .get::<Tag>(entity(&prefab, placed.turret.barrel))
            .is_none(),
        "the barrel gains nothing"
    );
}

/// An instance an author worked on: renamed, its light taken off the root,
/// its sight moved and tagged — the value and component overrides — and,
/// structurally, its barrel deleted and an entity of the author's hung under
/// the sight. Returns the muzzle's identity.
fn worked_on(placed: &mut Placed) -> PersistentId {
    let sight = placed.member(placed.turret.sight);
    let barrel = placed.member(placed.turret.barrel);
    placed.world.get_mut::<Name>(placed.root).expect("named").0 = "Gate turret".into();
    placed
        .world
        .remove_component::<Light>(placed.root)
        .expect("the light comes off");
    transform_mut(&mut placed.world, sight).translation = Vec3::new(0.0, 0.0, 3.0);
    placed
        .world
        .add_component(sight, Tag::from_iter(["armed"]))
        .expect("a tag attaches");
    placed.world.despawn_subtree(barrel);
    let muzzle = placed
        .world
        .spawn((Transform::identity(), Name::new("Muzzle")));
    placed.world.set_parent(muzzle, Some(sight));
    placed.world.mark_authored(muzzle).expect("authored")
}

/// Applying the whole instance writes every value override and every
/// component added to or removed from a member — but neither the member the
/// author deleted nor the entity they hung under the sight: those are the
/// instance's own structure.
#[test]
fn applying_the_instance_writes_its_values_and_components_not_its_structure() {
    let mut placed = placed();
    let muzzle = worked_on(&mut placed);

    let written = placed.apply(PrefabApply::Instance);

    assert!(
        !written.entities.contains(&muzzle) && !has_row(&written, muzzle),
        "the author's muzzle is not the prefab's"
    );
    let prefab = open(&written, &placed.library);
    assert_eq!(prefab.iter_entities().count(), 3, "root, barrel and sight");
    let root = entity(&prefab, placed.turret.root);
    let sight = entity(&prefab, placed.turret.sight);
    let barrel = entity(&prefab, placed.turret.barrel);
    assert_eq!(name(&prefab, root), "Gate turret");
    assert!(prefab.get::<Light>(root).is_none(), "the light comes off");
    assert_eq!(translation(&prefab, sight), Vec3::new(0.0, 0.0, 3.0));
    assert!(tagged(&prefab, sight), "the sight's tag");
    assert_eq!(
        translation(&prefab, barrel),
        Vec3::new(0.0, 1.0, 0.0),
        "the deleted barrel stays in the prefab, as it was"
    );
    assert_eq!(name(&prefab, barrel), "Barrel");
}

/// After the whole instance is applied, what was not written stays the
/// instance's override: saved against the new prefab and opened again, the
/// barrel is still deleted and the muzzle still hangs under the sight.
#[test]
fn structural_changes_stay_overrides_of_the_instance_after_an_apply() {
    let mut placed = placed();
    let muzzle = worked_on(&mut placed);

    let written = placed.apply(PrefabApply::Instance);
    let library = placed.library.clone().with(placed.prefab, written);
    let loaded = open(&save(&placed.world, &library), &library);

    assert_eq!(
        loaded.entity_with_id(PersistentId::within(placed.instance, placed.turret.barrel)),
        None,
        "the barrel stays deleted"
    );
    let sight = entity(
        &loaded,
        PersistentId::within(placed.instance, placed.turret.sight),
    );
    let muzzle = entity(&loaded, muzzle);
    assert_eq!(parent(&loaded, muzzle), Some(sight));
    assert_eq!(
        name(&loaded, entity(&loaded, placed.instance)),
        "Gate turret"
    );
}

/// Applying from a turret nested in a tower writes the turret's prefab, in
/// the turret's own ids — not the tower — and the raise the tower gives its
/// turret is not written with the barrel's field.
#[test]
fn applying_to_a_nested_instance_writes_the_inner_prefab() {
    let turret = turret();
    let turret_prefab = AssetUUID::new();
    let tower_prefab = AssetUUID::new();
    let library = Library::default().with(turret_prefab, turret.record.clone());
    let tower = tower(turret_prefab, &library);
    let library = library.with(tower_prefab, tower.record.clone());
    let mut world = World::new();
    let tower_root =
        instantiate_prefab(&mut world, tower_prefab, &library).expect("the tower instantiates");
    let nested = PersistentId::within(id(&world, tower_root), tower.turret_root);
    let barrel = entity(&world, PersistentId::within(nested, turret.barrel));
    transform_mut(&mut world, barrel).translation.x = 7.0;
    let instance = instance_of(&world, barrel, &library).expect("the barrel is the turret's");
    assert_eq!(instance.prefab, turret_prefab);

    let written = apply_to_prefab(
        &world,
        &instance,
        &field(barrel, "Transform", &["translation"]),
        &library,
    )
    .unwrap_or_else(|e| panic!("the apply is refused: {e}"));

    assert_eq!(written.entities[0], turret.root, "the turret's own root id");
    assert!(has_row(&written, turret.barrel), "the barrel's own id");
    assert_eq!(written.entities.len(), 3, "the turret's three, no tower");
    let prefab = open(&written, &library);
    assert_eq!(
        translation(&prefab, entity(&prefab, turret.barrel)),
        Vec3::new(7.0, 1.0, 0.0)
    );
    assert_eq!(
        translation(&prefab, entity(&prefab, turret.root)),
        Vec3::ZERO,
        "the tower's raise is the tower's"
    );
}

/// The prefab an apply writes never links to itself: the link the instance
/// root carries is the instance's, not the prefab's — the new prefab holds
/// no instance and no link, and spawns as before.
#[test]
fn an_applied_prefab_never_links_to_itself() {
    let mut placed = placed();
    placed.world.get_mut::<Name>(placed.root).expect("named").0 = "Gate turret".into();

    let written = placed.apply(PrefabApply::Instance);

    assert!(written.instances.is_empty(), "{:?}", written.instances);
    assert!(
        !holds_component(&written, "PrefabInstance"),
        "the prefab's root carries no link"
    );
    let library = placed.library.clone().with(placed.prefab, written);
    let mut spawned = World::new();
    let root = instantiate_prefab(&mut spawned, placed.prefab, &library)
        .unwrap_or_else(|e| panic!("the applied prefab does not spawn: {e}"));
    assert_eq!(spawned.iter_entities().count(), 3);
    assert_eq!(name(&spawned, root), "Gate turret");
}

/// Applying to a tower keeps the turret it holds as a link: the tower's file
/// names the turret prefab and holds no row of the turret's members, and an
/// override made on the nested turret becomes the tower's override of it.
#[test]
fn applying_to_an_outer_instance_keeps_its_nested_links() {
    let turret = turret();
    let turret_prefab = AssetUUID::new();
    let tower_prefab = AssetUUID::new();
    let library = Library::default().with(turret_prefab, turret.record.clone());
    let tower = tower(turret_prefab, &library);
    let library = library.with(tower_prefab, tower.record.clone());
    let mut world = World::new();
    let tower_root =
        instantiate_prefab(&mut world, tower_prefab, &library).expect("the tower instantiates");
    let nested = PersistentId::within(id(&world, tower_root), tower.turret_root);
    let barrel = entity(&world, PersistentId::within(nested, turret.barrel));
    transform_mut(&mut world, barrel).translation.x = 7.0;
    world.get_mut::<Name>(tower_root).expect("named").0 = "Keep".into();
    let instance = InstanceOf {
        root: tower_root,
        prefab: tower_prefab,
    };

    let written = apply_to_prefab(&world, &instance, &PrefabApply::Instance, &library)
        .unwrap_or_else(|e| panic!("the apply is refused: {e}"));

    assert_eq!(written.instances.len(), 1, "{:?}", written.instances);
    assert_eq!(written.instances[0].prefab, turret_prefab);
    assert_eq!(written.instances[0].root, tower.turret_root);
    assert!(
        !has_row(
            &written,
            PersistentId::within(tower.turret_root, turret.barrel)
        ),
        "the tower holds no row of the turret's barrel"
    );
    let prefab = open(&written, &library);
    let tower_root = entity(&prefab, written.entities[0]);
    assert_eq!(name(&prefab, tower_root), "Keep");
    assert_eq!(
        translation(
            &prefab,
            entity(
                &prefab,
                PersistentId::within(tower.turret_root, turret.barrel)
            )
        ),
        Vec3::new(7.0, 1.0, 0.0),
        "the barrel's push is the tower's override of its turret"
    );
    assert_eq!(
        translation(&prefab, entity(&prefab, tower.turret_root)),
        Vec3::new(0.0, 5.0, 0.0),
        "the tower still raises its turret"
    );
}

/// The point of applying: another instance of the same prefab, saved before
/// the apply and never touched, takes the applied value the next time its
/// scene is opened.
#[test]
fn another_instance_takes_an_applied_value_on_its_next_load() {
    let mut placed = placed();
    let other = instantiate_prefab(&mut placed.world, placed.prefab, &placed.library)
        .expect("a second turret instantiates");
    let other = id(&placed.world, other);
    let barrel = placed.member(placed.turret.barrel);
    transform_mut(&mut placed.world, barrel).translation.x = 5.0;
    let scene = save(&placed.world, &placed.library);

    let written = placed.apply(field(barrel, "Transform", &["translation"]));
    let library = placed.library.clone().with(placed.prefab, written);
    let loaded = open(&scene, &library);

    let other_barrel = entity(&loaded, PersistentId::within(other, placed.turret.barrel));
    assert_eq!(
        translation(&loaded, other_barrel),
        Vec3::new(5.0, 1.0, 0.0),
        "the other turret takes the applied push"
    );
    let barrel = entity(
        &loaded,
        PersistentId::within(placed.instance, placed.turret.barrel),
    );
    assert_eq!(translation(&loaded, barrel), Vec3::new(5.0, 1.0, 0.0));
}

/// An instance whose prefab can no longer be read has nothing to apply to:
/// the apply is refused.
#[test]
fn applying_to_an_instance_whose_prefab_is_gone_is_refused() {
    let mut placed = placed();
    let barrel = placed.member(placed.turret.barrel);
    transform_mut(&mut placed.world, barrel).translation.x = 5.0;

    let refused = apply_to_prefab(
        &placed.world,
        &placed.of(),
        &field(barrel, "Transform", &["translation"]),
        &Library::default(),
    );

    assert!(refused.is_err(), "{refused:?}");
}

/// The root's point light, to change in place.
fn point_light_mut(
    world: &mut World,
    entity: EntityId,
) -> &mut khora_core::renderer::light::PointLight {
    match &mut world.get_mut::<Light>(entity).expect("lit").light_type {
        khora_core::renderer::light::LightType::Point(point) => point,
        other => panic!("not a point light: {other:?}"),
    }
}

/// A path through an enum's variant writes the leaf it reaches inside the
/// variant's payload alone: the light's range and its colour's red, not the
/// intensity the instance also changed.
#[test]
fn applying_a_field_inside_an_enum_variant_writes_that_leaf_alone() {
    let mut placed = placed();
    let root = placed.root;
    let before = *point_light_mut(&mut placed.world, root);
    {
        let point = point_light_mut(&mut placed.world, root);
        point.range = before.range + 15.0;
        point.color.r = 0.25;
        point.intensity = before.intensity * 2.0;
    }

    let written = placed.apply(field(root, "Light", &["light_type", "Point", "range"]));
    let library = placed.library.clone().with(placed.prefab, written);
    let written = apply_to_prefab(
        &placed.world,
        &placed.of(),
        &field(root, "Light", &["light_type", "Point", "color", "r"]),
        &library,
    )
    .unwrap_or_else(|e| panic!("the apply is refused: {e}"));

    let mut prefab = open(&written, &library);
    let prefab_root = entity(&prefab, placed.turret.root);
    let point = *point_light_mut(&mut prefab, prefab_root);
    assert_eq!(point.range, before.range + 15.0);
    assert_eq!(point.color.r, 0.25);
    assert_eq!(point.color.g, before.color.g);
    assert_eq!(
        point.intensity, before.intensity,
        "the intensity is not applied"
    );
}

/// The barrel the author hung under an entity of their own inside the
/// instance has moved in the instance's structure, which the whole instance
/// does not write: the prefab keeps its barrel under its root, never under an
/// entity it does not hold.
#[test]
fn applying_the_instance_keeps_a_member_moved_under_an_author_entity_in_place() {
    let mut placed = placed();
    let sight = placed.member(placed.turret.sight);
    let barrel = placed.member(placed.turret.barrel);
    let muzzle = placed
        .world
        .spawn((Transform::identity(), Name::new("Muzzle")));
    placed.world.set_parent(muzzle, Some(sight));
    placed.world.mark_authored(muzzle).expect("authored");
    placed.world.set_parent(barrel, Some(muzzle));

    let written = placed.apply(PrefabApply::Instance);

    let prefab = open(&written, &placed.library);
    assert_eq!(
        parent(&prefab, entity(&prefab, placed.turret.barrel)),
        Some(entity(&prefab, placed.turret.root)),
        "the prefab's barrel left its root"
    );
}

/// The barrel hung under an entity of the author's shows its parent as an
/// override, and applying that component alone cannot write a parent the
/// prefab does not hold: the apply is refused, or the prefab keeps its
/// barrel under one of its own.
#[test]
fn applying_a_parent_naming_an_author_entity_never_orphans_the_member() {
    let mut placed = placed();
    let sight = placed.member(placed.turret.sight);
    let barrel = placed.member(placed.turret.barrel);
    let muzzle = placed
        .world
        .spawn((Transform::identity(), Name::new("Muzzle")));
    placed.world.set_parent(muzzle, Some(sight));
    placed.world.mark_authored(muzzle).expect("authored");
    placed.world.set_parent(barrel, Some(muzzle));

    let Ok(written) = apply_to_prefab(
        &placed.world,
        &placed.of(),
        &component(barrel, "Parent"),
        &placed.library,
    ) else {
        return;
    };

    let prefab = open(&written, &placed.library);
    let held = parent(&prefab, entity(&prefab, placed.turret.barrel));
    assert!(
        held.is_some_and(|parent| prefab.persistent_id(parent).is_some()),
        "the prefab's barrel hangs under nothing it holds: {held:?}"
    );
}
