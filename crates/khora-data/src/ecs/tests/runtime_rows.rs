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

//! Rows of run-time components through what storage does on its own:
//! compaction of the rows migrations leave behind, relayouts while such rows
//! exist, joins across domains — and the reads by key that must find nothing.

use khora_core::ecs::entity::EntityId;
use khora_core::math::Vec3;
use khora_core::script::ScriptValue;

use super::runtime_components::{declare, field, layout, patch, read, write};
use super::{Position, RenderId, Velocity};
use crate::ecs::query::Without;
use crate::ecs::{
    Component, ComponentKey, FieldKind, LossCause, RowError, SemanticDomain, Transform, World,
};

fn str_value(text: &str) -> ScriptValue {
    ScriptValue::Str(text.to_owned())
}

fn items(n: i64) -> ScriptValue {
    ScriptValue::Array(vec![ScriptValue::Int(n)])
}

/// A Spatial Rust component, to migrate rows with.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Spin(f32);
impl Component for Spin {}

/// A type no world registers.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Unregistered(u8);
impl Component for Unregistered {}

/// Six entities, each a `Transform` beside the declared Spatial components
/// `Badge { label: Value, n: Int }` and `Pouch { items: Value }`.
///
/// Entity `i` holds `t{i}`, `i`, `[i]`.
fn crowd() -> (World, ComponentKey, ComponentKey, Vec<EntityId>) {
    let mut world = World::new();
    world.register_component::<Spin>(SemanticDomain::Spatial);
    let badge = declare(
        &mut world,
        "Badge",
        SemanticDomain::Spatial,
        vec![
            field("label", FieldKind::Value, str_value("")),
            field("n", FieldKind::Int, ScriptValue::Int(0)),
        ],
    );
    let pouch = declare(
        &mut world,
        "Pouch",
        SemanticDomain::Spatial,
        vec![field("items", FieldKind::Value, ScriptValue::Array(vec![]))],
    );
    let entities = (0..6i64)
        .map(|i| {
            let entity = world.spawn(Transform::from_translation(Vec3::new(i as f32, 0.0, 0.0)));
            world
                .add_runtime_component(
                    entity,
                    badge,
                    &patch(&[
                        ("label", str_value(&format!("t{i}"))),
                        ("n", ScriptValue::Int(i)),
                    ]),
                )
                .expect("Badge attaches");
            world
                .add_runtime_component(entity, pouch, &patch(&[("items", items(i))]))
                .expect("Pouch attaches");
            entity
        })
        .collect();
    (world, badge, pouch, entities)
}

/// Entity `i` of the crowd still holds what it was given.
fn assert_intact(
    world: &World,
    badge: ComponentKey,
    pouch: ComponentKey,
    entity: EntityId,
    i: i64,
) {
    assert_eq!(
        read(world, entity, badge, "label"),
        Some(str_value(&format!("t{i}"))),
        "entity {i}"
    );
    assert_eq!(
        read(world, entity, badge, "n"),
        Some(ScriptValue::Int(i)),
        "entity {i}"
    );
    assert_eq!(
        read(world, entity, pouch, "items"),
        Some(items(i)),
        "entity {i}"
    );
    assert_eq!(
        world.get::<Transform>(entity).map(|t| t.translation.x),
        Some(i as f32),
        "entity {i}"
    );
}

#[test]
fn compaction_keeps_runtime_rows_with_value_fields() {
    let (mut world, badge, pouch, crowd) = crowd();
    // Migrations leave rows behind in two pages.
    for i in [0, 2, 4] {
        world
            .add_component(crowd[i], Spin(i as f32))
            .expect("Spin attaches");
    }
    assert!(world.remove_component_by_key(crowd[1], badge));
    assert!(world.despawn(crowd[3]));
    world.run_compaction(usize::MAX);

    for i in [0, 2, 4, 5] {
        assert_intact(&world, badge, pouch, crowd[i], i as i64);
    }
    assert!(world.row(crowd[1], badge).is_none());
    assert_eq!(read(&world, crowd[1], pouch, "items"), Some(items(1)));

    // And back, leaving rows behind in the Spin page.
    for i in [0, 2, 4] {
        world
            .remove_component::<Spin>(crowd[i])
            .expect("Spin detaches");
    }
    world.run_compaction(usize::MAX);
    for i in [0, 2, 4, 5] {
        assert_intact(&world, badge, pouch, crowd[i], i as i64);
    }
}

#[test]
fn relayout_rebuilds_rows_left_behind_so_compaction_and_migrations_follow() {
    let (mut world, badge, pouch, crowd) = crowd();
    // Rows of Badge left behind in its page, not yet compacted.
    world
        .add_component(crowd[0], Spin(0.0))
        .expect("Spin attaches");
    world
        .add_component(crowd[2], Spin(2.0))
        .expect("Spin attaches");
    let extra = ScriptValue::Vec3(Vec3::new(1.0, 2.0, 3.0));
    let report = world
        .relayout(
            badge,
            layout(vec![
                field("extra", FieldKind::Vec3, extra.clone()),
                field("label", FieldKind::Value, str_value("")),
                field("n", FieldKind::Int, ScriptValue::Int(0)),
            ]),
        )
        .expect("Badge is relaid out");
    assert_eq!(report.defaulted, vec!["extra".to_owned()]);

    world.run_compaction(usize::MAX);
    for (i, entity) in crowd.iter().enumerate() {
        assert_intact(&world, badge, pouch, *entity, i as i64);
        assert_eq!(read(&world, *entity, badge, "extra"), Some(extra.clone()));
    }

    // Migrations after the relayout copy the new rows.
    world
        .remove_component::<Spin>(crowd[0])
        .expect("Spin detaches");
    world
        .add_component(crowd[1], Spin(1.0))
        .expect("Spin attaches");
    assert!(world.remove_component_by_key(crowd[4], pouch));
    assert!(world.despawn(crowd[5]));
    world.run_compaction(usize::MAX);
    for (i, entity) in crowd.iter().enumerate().take(4) {
        assert_intact(&world, badge, pouch, *entity, i as i64);
    }
    assert_eq!(
        read(&world, crowd[4], badge, "n"),
        Some(ScriptValue::Int(4))
    );

    let moved = ScriptValue::Vec3(Vec3::new(9.0, 9.0, 9.0));
    write(&mut world, crowd[1], badge, "extra", moved.clone()).expect("a Vec3 fits");
    assert_eq!(read(&world, crowd[1], badge, "extra"), Some(moved));
    assert_eq!(read(&world, crowd[2], badge, "extra"), Some(extra));
}

#[test]
fn relayout_drops_and_retypes_value_fields_down_to_none_and_back() {
    let (mut world, badge, pouch, crowd) = crowd();
    world
        .add_component(crowd[0], Spin(0.0))
        .expect("Spin attaches");

    let report = world
        .relayout(
            badge,
            layout(vec![field("label", FieldKind::Int, ScriptValue::Int(-1))]),
        )
        .expect("Badge is relaid out");
    assert_eq!(report.dropped, vec!["n".to_owned()]);
    assert_eq!(report.defaulted, vec!["label".to_owned()]);
    for entity in &crowd {
        assert_eq!(
            read(&world, *entity, badge, "label"),
            Some(ScriptValue::Int(-1))
        );
        assert_eq!(read(&world, *entity, badge, "n"), None);
    }

    // No field at all: a tag, through a despawn, a migration and compaction.
    world
        .relayout(badge, layout(Vec::new()))
        .expect("Badge becomes a tag");
    assert!(world.despawn(crowd[1]));
    world
        .add_component(crowd[2], Spin(2.0))
        .expect("Spin attaches");
    world.run_compaction(usize::MAX);
    assert!(world.row(crowd[2], badge).is_some());

    world
        .relayout(
            badge,
            layout(vec![field("label", FieldKind::Value, str_value("back"))]),
        )
        .expect("Badge has a field again");
    for i in [0, 2, 3, 4, 5] {
        assert_eq!(
            read(&world, crowd[i], badge, "label"),
            Some(str_value("back"))
        );
        assert_eq!(
            read(&world, crowd[i], pouch, "items"),
            Some(items(i as i64))
        );
    }
}

#[test]
fn relayout_moves_its_domain_epoch() {
    let (mut world, badge, _, _) = crowd();
    // The clock moves past the setup, so the write below carries a later
    // tick than the rows it finds.
    world.end_run();
    let before = world.domain_epoch(SemanticDomain::Spatial);
    world
        .relayout(
            badge,
            layout(vec![field("n", FieldKind::Int, ScriptValue::Int(0))]),
        )
        .expect("Badge is relaid out");
    assert!(world.domain_epoch(SemanticDomain::Spatial) > before);
}

#[test]
fn filters_and_options_see_rows_beside_runtime_components() {
    let mut world = World::new();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<Velocity>(SemanticDomain::Physics);
    let mark = declare(
        &mut world,
        "Mark",
        SemanticDomain::Spatial,
        vec![field("v", FieldKind::Value, str_value("x"))],
    );
    let push = declare(
        &mut world,
        "Push",
        SemanticDomain::Physics,
        vec![field("v", FieldKind::Int, ScriptValue::Int(3))],
    );
    let a = world.spawn(Position(1));
    let b = world.spawn((Position(2), Velocity(20)));
    let c = world.spawn(Velocity(30));
    for (entity, key) in [(a, mark), (b, mark), (b, push), (c, push)] {
        world
            .add_runtime_component(entity, key, &ScriptValue::Unit)
            .expect("attaches");
    }

    let mut got: Vec<(i32, Option<i32>)> = world
        .query::<(&Position, Option<&Velocity>)>()
        .map(|(p, v)| (p.0, v.map(|v| v.0)))
        .collect();
    got.sort();
    assert_eq!(got, vec![(1, None), (2, Some(20))]);
    let got: Vec<i32> = world
        .query::<(&Position, Without<Velocity>)>()
        .map(|(p, _)| p.0)
        .collect();
    assert_eq!(got, vec![1]);
    let got: Vec<i32> = world
        .query::<(&Velocity, Without<Position>)>()
        .map(|(v, _)| v.0)
        .collect();
    assert_eq!(got, vec![30]);
    assert_eq!(
        world.query::<(&Position, Without<Unregistered>)>().count(),
        2
    );
    assert_eq!(
        world
            .query::<(&Position, Option<&Unregistered>)>()
            .filter(|(_, u)| u.is_none())
            .count(),
        2
    );

    // The clock moves past the setup, so the write below carries a later
    // tick than the rows it finds.
    world.end_run();
    let before = world.domain_epoch(SemanticDomain::Physics);
    for mut v in world.query_mut::<&mut Velocity>() {
        v.0 += 1;
    }
    assert!(world.domain_epoch(SemanticDomain::Physics) > before);
    assert_eq!(world.get::<Velocity>(c), Some(&Velocity(31)));
    assert_eq!(read(&world, c, push, "v"), Some(ScriptValue::Int(3)));
    assert_eq!(read(&world, b, mark, "v"), Some(str_value("x")));
}

#[test]
fn a_transversal_join_reaches_rows_beside_runtime_components() {
    let mut world = World::new();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<Velocity>(SemanticDomain::Physics);
    let mark = declare(
        &mut world,
        "Mark",
        SemanticDomain::Spatial,
        vec![field("v", FieldKind::Value, str_value("x"))],
    );
    let push = declare(
        &mut world,
        "Push",
        SemanticDomain::Physics,
        vec![field("v", FieldKind::Int, ScriptValue::Int(3))],
    );
    // Position and Velocity in two pages: a join across domains.
    let entities: Vec<EntityId> = (0..5)
        .map(|i| {
            let entity = world.spawn(Position(i));
            world
                .add_component(entity, Velocity(i * 10))
                .expect("Velocity attaches");
            if i % 2 == 0 {
                world
                    .add_runtime_component(entity, mark, &ScriptValue::Unit)
                    .expect("Mark attaches");
            }
            if i % 3 == 0 {
                world
                    .add_runtime_component(entity, push, &ScriptValue::Unit)
                    .expect("Push attaches");
            }
            entity
        })
        .collect();
    assert!(world.despawn(entities[1]));
    world.run_compaction(usize::MAX);

    let mut got: Vec<(i32, i32)> = world
        .query::<(&Position, &Velocity)>()
        .map(|(p, v)| (p.0, v.0))
        .collect();
    got.sort();
    assert_eq!(got, vec![(0, 0), (2, 20), (3, 30), (4, 40)]);
    for (mut position, velocity) in world.query_mut::<(&mut Position, &Velocity)>() {
        position.0 += velocity.0;
    }
    let mut got: Vec<i32> = world.query::<&Position>().map(|p| p.0).collect();
    got.sort();
    assert_eq!(got, vec![0, 22, 33, 44]);
    assert_eq!(
        read(&world, entities[3], push, "v"),
        Some(ScriptValue::Int(3))
    );
    assert_eq!(read(&world, entities[4], mark, "v"), Some(str_value("x")));
}

#[test]
fn a_recycled_entity_does_not_see_its_predecessor_s_row() {
    let (mut world, badge, _, crowd) = crowd();
    let dead = crowd[0];
    assert!(world.despawn(dead));
    assert!(world.row(dead, badge).is_none());
    assert!(world.row_mut(dead, badge).is_none());

    let recycled = (0..crowd.len() + 1)
        .map(|_| world.spawn(()))
        .find(|entity| entity.index == dead.index)
        .expect("the freed index is reused");
    assert_ne!(recycled.generation, dead.generation);
    assert!(world.row(dead, badge).is_none());
    assert!(
        world.row(recycled, badge).is_none(),
        "a new entity holds nothing"
    );
    assert_eq!(
        world.add_runtime_component(dead, badge, &ScriptValue::Unit),
        Err(RowError::NoSuchEntity(dead))
    );
    world
        .add_runtime_component(recycled, badge, &ScriptValue::Unit)
        .expect("Badge attaches");
    assert_eq!(
        read(&world, recycled, badge, "n"),
        Some(ScriptValue::Int(0))
    );
}

/// A derived Rust component with a field a script reaches, a skipped one and
/// one of a type no script can hold.
#[derive(Debug, Clone, Default, PartialEq, khora_macros::Component)]
struct Gearbox {
    ratio: f32,
    #[component(skip)]
    wear: u32,
    revs: u64,
}

/// A Rust row read by key reads the fields a script reaches, and only those.
///
/// A skipped field has no slot; a field of a type no script can hold, or a
/// slot past the last field, reads nothing; an entity lacking the component
/// has no row of it.
#[test]
fn a_rust_row_reads_only_the_fields_a_script_reaches() {
    let (mut world, _, _, crowd) = crowd();
    let transform = ComponentKey::of::<Transform>();
    let translation = ScriptValue::Vec3(Vec3::new(2.0, 0.0, 0.0));
    let row = world.row(crowd[2], transform).expect("a Transform row");
    assert_eq!(&*row.vtable().name, "Transform");
    assert_eq!(row.field(0), Some(translation.clone()));
    assert_eq!(row.field(3), None, "Transform has three fields");
    let row = world.row_mut(crowd[2], transform).expect("a Transform row");
    assert_eq!(row.field(0), Some(translation));
    assert_eq!(row.field(3), None);

    world.register_component::<Gearbox>(SemanticDomain::Spatial);
    world
        .add_component(
            crowd[2],
            Gearbox {
                ratio: 3.5,
                wear: 9,
                revs: 7,
            },
        )
        .expect("Gearbox attaches");
    let gearbox = ComponentKey::of::<Gearbox>();
    let columns = world
        .components()
        .vtable(gearbox)
        .expect("registered")
        .columns
        .clone();
    assert_eq!(
        columns.field_slot("wear"),
        None,
        "a skipped field has no slot"
    );
    let ratio = columns.field_slot("ratio").expect("ratio has a slot");
    let revs = columns.field_slot("revs").expect("revs has a slot");
    let row = world.row(crowd[2], gearbox).expect("a Gearbox row");
    assert_eq!(row.field(ratio), Some(ScriptValue::Float(3.5)));
    assert_eq!(row.field(revs), None, "a u64 is no script value");
    assert_eq!(row.field(2), None, "two fields reach a script");

    assert!(
        world.row(crowd[2], ComponentKey::of::<Spin>()).is_none(),
        "an entity lacking the component has no row of it"
    );
}

#[test]
fn a_refused_runtime_add_changes_nothing() {
    let (mut world, badge, _, _) = crowd();
    let entity = world.spawn(Transform::identity());
    let pages = world.storage.pages.len();
    let epoch = world.domain_epoch(SemanticDomain::Spatial);
    let spin = ComponentKey::of::<Spin>();
    let nobody = ComponentKey::named("Nobody");

    assert_eq!(
        world.add_runtime_component(entity, badge, &patch(&[("nope", ScriptValue::Int(1))])),
        Err(RowError::UnknownField {
            field: "nope".to_owned()
        })
    );
    assert_eq!(
        world.add_runtime_component(entity, badge, &ScriptValue::Int(1)),
        Err(RowError::NotFields {
            found: ScriptValue::Int(1).type_name().to_owned()
        })
    );
    assert_eq!(
        world.add_runtime_component(entity, spin, &ScriptValue::Unit),
        Err(RowError::Unknown(spin)),
        "a Rust key is not a declared component's"
    );
    assert_eq!(
        world.add_runtime_component(entity, nobody, &ScriptValue::Unit),
        Err(RowError::Unknown(nobody))
    );

    assert!(world.row(entity, badge).is_none());
    assert_eq!(world.storage.pages.len(), pages, "no page was made");
    assert_eq!(
        world.domain_epoch(SemanticDomain::Spatial),
        epoch,
        "nothing changed"
    );
    assert_eq!(world.get::<Transform>(entity), Some(&Transform::identity()));
}

/// **A relayout reports the values of live rows only**: a row a migration left
/// behind belongs to no entity, and its value is nobody's loss.
#[test]
fn a_relayout_reports_no_value_of_a_row_left_behind() {
    let (mut world, badge, _, crowd) = crowd();
    world
        .add_component(crowd[0], Spin(0.0))
        .expect("Spin attaches");

    let report = world
        .relayout(
            badge,
            layout(vec![field("label", FieldKind::Value, str_value(""))]),
        )
        .expect("Badge is relaid out");

    let mut reported: Vec<(EntityId, ScriptValue)> = report
        .lost_in("n", LossCause::FieldDropped)
        .map(|lost| (lost.entity, lost.was.clone()))
        .collect();
    reported.sort_by_key(|(entity, _)| entity.index);
    let mut live: Vec<(EntityId, ScriptValue)> = crowd
        .iter()
        .enumerate()
        .map(|(i, entity)| (*entity, ScriptValue::Int(i as i64)))
        .collect();
    live.sort_by_key(|(entity, _)| entity.index);
    assert_eq!(
        reported, live,
        "each live row once, the row left behind never"
    );
}

/// **A relayout reports no value of a component its entity dropped**: the
/// entity leaves the domain of a declared component sharing its row with
/// another domain's, so the row stays live, but the declared value is no one's.
#[test]
fn a_relayout_reports_no_value_of_a_component_dropped_from_a_shared_row() {
    let mut world = World::new();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<RenderId>(SemanticDomain::Render);
    let mark = declare(
        &mut world,
        "Mark",
        SemanticDomain::Spatial,
        vec![field("n", FieldKind::Int, ScriptValue::Int(0))],
    );
    let entity = world.spawn((Position(1), RenderId(10)));
    world
        .add_runtime_component(entity, mark, &patch(&[("n", ScriptValue::Int(5))]))
        .expect("Mark attaches");
    world
        .remove_component_domain::<Position>(entity)
        .expect("the entity leaves the Spatial domain");
    assert_eq!(read(&world, entity, mark, "n"), None, "Mark is gone");

    let report = world
        .relayout(
            mark,
            layout(vec![field("m", FieldKind::Int, ScriptValue::Int(0))]),
        )
        .expect("Mark is relaid out");

    assert!(report.lost_nothing(), "{report:?}");
}

/// The relayout of `Badge` that turns its `n` into a `float`.
fn badge_with_a_float_n() -> crate::ecs::PackedLayout {
    layout(vec![
        field("label", FieldKind::Value, str_value("")),
        field("n", FieldKind::Float, ScriptValue::Float(0.5)),
    ])
}

/// **A field is defaulted where every live row lost its value, whatever a row
/// left behind held**: a migration leaves `crowd[0]`'s exact `0` behind, then
/// every live row is given 2^24 + 1, which no `float` holds. Relaid out to a
/// `float`, every live row loses it, so `n` is defaulted; the row left behind,
/// which would have kept its value, counts for no one.
#[test]
fn a_field_every_live_row_lost_is_defaulted_whatever_a_row_left_behind_held() {
    let (mut world, badge, _, crowd) = crowd();
    world
        .add_component(crowd[0], Spin(0.0))
        .expect("Spin attaches");
    for &entity in &crowd {
        write(&mut world, entity, badge, "n", ScriptValue::Int(16_777_217))
            .expect("an int fits `n`");
    }

    let report = world
        .relayout(badge, badge_with_a_float_n())
        .expect("Badge is relaid out");

    assert_eq!(report.lost_in("n", LossCause::Inexact).count(), crowd.len());
    assert_eq!(report.defaulted, vec!["n".to_owned()], "{report:?}");
}

/// **A relayout after a domain move reports each entity's value once**: `Mark`
/// moves from `Spatial` to `Render` on an entity whose row both domains share,
/// leaving its old row behind in the page it lands in again; relaid out
/// without `n`, the entity loses its value once.
#[test]
fn a_relayout_after_a_domain_move_reports_each_value_once() {
    let mut world = World::new();
    world.register_component::<Position>(SemanticDomain::Spatial);
    world.register_component::<RenderId>(SemanticDomain::Render);
    let mark = declare(
        &mut world,
        "Mark",
        SemanticDomain::Spatial,
        vec![field("n", FieldKind::Int, ScriptValue::Int(0))],
    );
    let entity = world.spawn((Position(1), RenderId(10)));
    world
        .add_runtime_component(entity, mark, &patch(&[("n", ScriptValue::Int(5))]))
        .expect("Mark attaches");
    world
        .move_component_domain(mark, SemanticDomain::Render)
        .expect("Mark moves");
    assert_eq!(read(&world, entity, mark, "n"), Some(ScriptValue::Int(5)));

    let report = world
        .relayout(
            mark,
            layout(vec![field("m", FieldKind::Int, ScriptValue::Int(0))]),
        )
        .expect("Mark is relaid out");

    let lost: Vec<(EntityId, ScriptValue)> = report
        .lost_in("n", LossCause::FieldDropped)
        .map(|lost| (lost.entity, lost.was.clone()))
        .collect();
    assert_eq!(lost, vec![(entity, ScriptValue::Int(5))], "{report:?}");
}
