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

//! The page as the unit a scene is written in: a capture reads the world's
//! pages, a load builds them, and a record whose pages do not fit together is
//! refused before the world is touched.

use std::collections::HashSet;

use khora_core::math::Vec3;
use khora_data::ecs::{
    Camera, EcsMaintenance, GlobalTransform, Name, Script, Tag, Transform, World,
};
use khora_data::scene::record::{EntityRef, ReferenceWriter, ReportKind};
use khora_data::scene::{component_to_record, registration_named};

use super::failures::{fingerprint, occupied_world};
use super::*;

/// Writes an entity by the identity `world` knows it by.
struct ById<'w>(&'w World);

impl ReferenceWriter for ById<'_> {
    fn write_entity(&mut self, entity: EntityId) -> EntityRef {
        self.0
            .persistent_id(entity)
            .map_or(EntityRef::Outside, EntityRef::Id)
    }
}

/// Asserts each page of `record` is row-aligned with `world`: one column per
/// component, one value per row, and each value the component that row's
/// entity holds.
fn assert_row_aligned(world: &World, record: &SceneRecord) {
    for page in &record.pages {
        assert_eq!(
            page.columns.len(),
            page.components.len(),
            "one column per component: {:?}",
            page.components
        );
        for (name, column) in page.components.iter().zip(&page.columns) {
            assert_eq!(column.len(), page.rows.len(), "`{name}`: one value per row");
            let reg = registration_named(name)
                .unwrap_or_else(|| panic!("`{name}` is not a registered component"));
            for (id, value) in page.rows.iter().zip(column) {
                let entity = world
                    .entity_with_id(*id)
                    .unwrap_or_else(|| panic!("a row of {id:?}, which the world does not hold"));
                let expected = component_to_record(reg, world, entity, &mut ById(world))
                    .unwrap_or_else(|| panic!("{id:?} has no `{name}`, yet holds a row with one"))
                    .unwrap_or_else(|e| panic!("`{name}` of {id:?} does not write: {e}"));
                assert_eq!(
                    value, &expected,
                    "`{name}` of {id:?} is not its row's value"
                );
            }
        }
    }
}

/// A tag set holding `label`.
fn tag(label: &str) -> Tag {
    let mut tag = Tag::new();
    tag.insert(label);
    tag
}

/// The page records holding a row of `id`.
fn pages_of(record: &SceneRecord, id: PersistentId) -> Vec<&PageRecord> {
    record
        .pages
        .iter()
        .filter(|page| page.rows.contains(&id))
        .collect()
}

/// Runs a maintenance pass that may compact every page and returns how many
/// pages it had to: the pages a migration left orphan rows in.
fn pages_to_compact(world: &mut World) -> usize {
    let mut maintenance = EcsMaintenance::with_budget(usize::MAX);
    maintenance.tick(world);
    maintenance.last_compacted_count()
}

/// A capture reads the world page by page. Entities of two kinds — spawned
/// interleaved — come out as one page record per kind: its components sorted
/// by name, its rows in the order the page holds them, and each column
/// holding, row for row, the value of that row's entity.
#[test]
fn a_capture_groups_rows_by_page() {
    let mut world = World::new();
    let at = |x: f32| Transform::from_translation(Vec3::new(x, 0.0, 0.0));
    let a1 = world.spawn((at(1.0), Name::new("a1")));
    let b1 = world.spawn((at(2.0), Name::new("b1"), tag("b1")));
    let a2 = world.spawn((at(3.0), Name::new("a2")));
    let b2 = world.spawn((at(4.0), Name::new("b2"), tag("b2")));
    let a3 = world.spawn((at(5.0), Name::new("a3")));
    let id = |entity| {
        world
            .persistent_id(entity)
            .expect("a live entity has an id")
    };

    let record = capture_world(&world).expect("captures");

    assert_eq!(
        record.pages.len(),
        2,
        "one page record per saved signature: {:?}",
        record.pages
    );
    for page in &record.pages {
        let mut sorted = page.components.clone();
        sorted.sort();
        assert_eq!(page.components, sorted, "a signature is sorted by name");
    }
    let plain = record
        .pages
        .iter()
        .find(|page| page.components == ["Name", "Transform"])
        .unwrap_or_else(|| panic!("no [Name, Transform] page: {:?}", record.pages));
    let tagged = record
        .pages
        .iter()
        .find(|page| page.components == ["Name", "Tag", "Transform"])
        .unwrap_or_else(|| panic!("no [Name, Tag, Transform] page: {:?}", record.pages));
    assert_eq!(
        plain.rows,
        vec![id(a1), id(a2), id(a3)],
        "rows in page order"
    );
    assert_eq!(tagged.rows, vec![id(b1), id(b2)], "rows in page order");
    assert_row_aligned(&world, &record);

    let listed: HashSet<_> = record.entities.iter().copied().collect();
    assert_eq!(record.entities.len(), 5, "every entity listed once");
    assert_eq!(
        listed,
        [a1, b1, a2, b2, a3]
            .map(id)
            .into_iter()
            .collect::<HashSet<_>>()
    );
}

/// What a page record groups is what a scene saves, not how the world
/// happens to store it: pages that differ only by a component the engine
/// derives hold the same saved components, and land in one page record —
/// each storage page's rows kept in their order.
#[test]
fn pages_with_the_same_saved_components_share_a_page_record() {
    let mut world = World::new();
    let a1 = world.spawn((Transform::identity(), Name::new("a1")));
    let derived = world.spawn((
        Transform::identity(),
        Name::new("derived"),
        GlobalTransform::identity(),
    ));
    let a2 = world.spawn((Transform::identity(), Name::new("a2")));
    let id = |entity| world.persistent_id(entity).expect("id");

    let record = capture_world(&world).expect("captures");
    assert_eq!(record.pages.len(), 1, "{:?}", record.pages);
    let page = &record.pages[0];
    assert_eq!(page.components, ["Name", "Transform"]);
    assert_eq!(page.rows.len(), 3, "each entity once: {:?}", page.rows);
    assert_eq!(
        page.rows.iter().copied().collect::<HashSet<_>>(),
        [a1, derived, a2]
            .map(id)
            .into_iter()
            .collect::<HashSet<_>>()
    );
    let position = |entity| page.rows.iter().position(|row| *row == id(entity));
    assert!(
        position(a1) < position(a2),
        "rows of one page keep the page's order: {:?}",
        page.rows
    );
    assert_row_aligned(&world, &record);
}

/// A page may hold rows its entities have left — a component added migrates
/// the entity and leaves the old row for compaction. A capture reads only the
/// rows that are live: the moved entity is recorded once, in its new page.
#[test]
fn a_capture_reads_only_live_rows() {
    let mut world = World::new();
    let moved = world.spawn(Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)));
    let stayed = world.spawn(Transform::identity());
    world
        .add_component(moved, Name::new("moved"))
        .expect("a name attaches");
    let id = |entity| world.persistent_id(entity).expect("id");

    let record = capture_world(&world).expect("captures");
    let rows_of_moved: Vec<_> = record
        .pages
        .iter()
        .flat_map(|page| page.rows.iter())
        .filter(|row| **row == id(moved))
        .collect();
    assert_eq!(
        rows_of_moved.len(),
        1,
        "the row it left is not recorded: {:?}",
        record.pages
    );
    let moved_pages = pages_of(&record, id(moved));
    assert_eq!(moved_pages[0].components, ["Name", "Transform"]);
    let stayed_pages = pages_of(&record, id(stayed));
    assert_eq!(stayed_pages.len(), 1);
    assert_eq!(stayed_pages[0].components, ["Transform"]);
    assert_row_aligned(&world, &record);
}

/// A world of the same saved components spread over several storage pages —
/// spawned with them, spawned with a derived one besides, given one after
/// spawning — is recorded as one page record, and loads as one page: capture
/// the loaded world again and the rows come back as that one page, in the
/// order the record listed them.
#[test]
fn entities_with_the_same_components_load_into_one_page() {
    let mut src = World::new();
    let plain = src.spawn((Transform::identity(), Name::new("plain")));
    let derived = src.spawn((
        Transform::identity(),
        Name::new("derived"),
        GlobalTransform::identity(),
    ));
    let moved = src.spawn(Transform::identity());
    src.add_component(moved, Name::new("moved"))
        .expect("a name attaches");
    for entity in [plain, derived, moved] {
        src.mark_authored(entity).expect("authored");
    }

    let record = capture_world(&src).expect("captures");
    assert_eq!(record.pages.len(), 1, "{:?}", record.pages);

    for (name, encoding) in every_encoding() {
        let (dst, _) = reload(&src, name, encoding);
        let again = capture_world(&dst).expect("the loaded world captures");
        assert_eq!(again.pages.len(), 1, "{name}: {:?}", again.pages);
        assert_eq!(
            again.pages[0].components, record.pages[0].components,
            "{name}"
        );
        assert_eq!(
            again.pages[0].rows, record.pages[0].rows,
            "{name}: the rows were built in record order"
        );
        assert_same_world(&src, &dst, name);
    }
}

/// A load builds each page in place — no entity migrated from page to page on
/// the way — so it leaves nothing for compaction: a maintenance pass right
/// after it has no page to compact. (A migration in the same world does leave
/// one, which is what makes the count worth reading.)
#[test]
fn loading_leaves_no_orphan_rows() {
    let mut control = World::new();
    let migrated = control.spawn(Transform::identity());
    control
        .add_component(migrated, Name::new("migrated"))
        .expect("a name attaches");
    assert!(
        pages_to_compact(&mut control) > 0,
        "a migration leaves a page to compact"
    );

    let mut src = World::new();
    src.spawn((Transform::identity(), Name::new("plain")));
    src.spawn((Transform::identity(), Name::new("tagged"), tag("t")));
    let eye = src.spawn((Transform::identity(), Name::new("eye")));
    src.add_component(
        eye,
        Camera::new_perspective(std::f32::consts::FRAC_PI_3, 1.5, 0.05, 500.0),
    )
    .expect("a camera attaches");
    src.spawn(Script {
        module: "ai/idle.erg".into(),
        behavior: "Idle".into(),
        fields: vec![],
    });
    src.spawn(GlobalTransform::identity());

    for (name, encoding) in every_encoding() {
        let (mut dst, _) = reload(&src, name, encoding);
        assert_eq!(
            pages_to_compact(&mut dst),
            0,
            "{name}: the load left orphan rows behind"
        );
    }
}

/// The same holds for a hierarchy: `Children` is derived from `Parent` when
/// the load completes, and deriving it must not migrate a parent out of the
/// page the load built for it.
#[test]
fn loading_a_hierarchy_leaves_no_orphan_rows() {
    let mut src = World::new();
    let root = src.spawn((Transform::identity(), Name::new("Root")));
    let child = src.spawn((Transform::identity(), Name::new("Child")));
    src.set_parent(child, Some(root));

    for (name, encoding) in every_encoding() {
        let (mut dst, _) = reload(&src, name, encoding);
        assert_eq!(
            pages_to_compact(&mut dst),
            0,
            "{name}: the load left orphan rows behind"
        );
    }
}

/// CRPECS keeps an entity's components in one page per domain. One given a
/// component of another domain after spawning sits in two pages — two page
/// records — and comes back with every component.
#[test]
fn an_entity_spanning_two_pages_round_trips() {
    let mut src = World::new();
    let eye = src.spawn((
        Transform::from_translation(Vec3::new(3.0, 4.0, 5.0)),
        Name::new("Eye"),
    ));
    src.add_component(
        eye,
        Camera::new_perspective(std::f32::consts::FRAC_PI_3, 1.5, 0.05, 500.0),
    )
    .expect("a camera attaches");
    src.spawn((Transform::identity(), Name::new("Plain")));
    let id = src.mark_authored(eye).expect("authored");

    let record = capture_world(&src).expect("captures");
    let pages = pages_of(&record, id);
    assert_eq!(
        pages.len(),
        2,
        "one page record per page: {:?}",
        record.pages
    );
    let mut names: Vec<&str> = pages
        .iter()
        .flat_map(|page| page.components.iter().map(String::as_str))
        .collect();
    names.sort_unstable();
    assert_eq!(names, ["Camera", "Name", "Transform"]);
    assert_row_aligned(&src, &record);

    for (name, encoding) in every_encoding() {
        let (dst, applied) = reload(&src, name, encoding);
        assert!(applied.report.is_clean(), "{name}: {:?}", applied.report);
        assert_same_world(&src, &dst, name);
        let twin = dst.entity_with_id(id).expect("loaded");
        assert!(dst.get::<Transform>(twin).is_some(), "{name}: Transform");
        assert!(dst.get::<Name>(twin).is_some(), "{name}: Name");
        assert!(dst.get::<Camera>(twin).is_some(), "{name}: Camera");
    }
}

/// An entity with nothing worth saving — only what the engine derives — is in
/// no page record, but it exists: it is listed, and a load brings it back
/// under its identity.
#[test]
fn an_entity_without_saved_components_survives() {
    let mut src = World::new();
    let bare = src.spawn(GlobalTransform::identity());
    let named = src.spawn((Transform::identity(), Name::new("Named")));
    let bare_id = src.mark_authored(bare).expect("authored");
    src.mark_authored(named).expect("authored");

    let record = capture_world(&src).expect("captures");
    assert!(record.entities.contains(&bare_id), "it is listed");
    assert!(pages_of(&record, bare_id).is_empty(), "it holds no row");

    for (name, encoding) in every_encoding() {
        let (dst, applied) = reload(&src, name, encoding);
        assert!(
            applied.entities.iter().any(|(id, _)| *id == bare_id),
            "{name}: it is brought in"
        );
        let twin = dst
            .entity_with_id(bare_id)
            .unwrap_or_else(|| panic!("{name}: the bare entity did not survive"));
        assert!(dst.contains(twin), "{name}: it is alive");
        assert_eq!(dst.iter_entities().count(), 2, "{name}");
        assert!(dst.get::<Name>(twin).is_none(), "{name}: it gained nothing");
    }
}

/// An entity whose only saved component has been retired since is still an
/// entity: the load skips the component, reports it, and brings the entity in.
#[test]
fn an_entity_whose_only_component_is_retired_still_loads() {
    let mut src = World::new();
    let lone = src.spawn(GlobalTransform::identity());
    let lone_id = src.mark_authored(lone).expect("authored");
    let mut record = capture_world(&src).expect("captures");
    record.pages.push(PageRecord {
        components: vec!["Compass".into()],
        rows: vec![lone_id],
        columns: vec![vec![Record::Struct {
            name: "SerializableCompass".into(),
            fields: vec![("heading".into(), Record::F32(90.0))],
        }]],
    });

    let mut dst = World::new();
    let applied = apply(&mut dst, &record, Identity::Keep).expect("a retired type is no error");
    let twin = dst.entity_with_id(lone_id).expect("the entity loaded");
    assert!(dst.contains(twin));
    assert_eq!(dst.iter_entities().count(), 1);
    assert!(
        applied
            .report
            .entries
            .iter()
            .any(|entry| entry.kind == ReportKind::Retired && entry.entity == Some(lone_id)),
        "{:?}",
        applied.report
    );
    let again = capture_world(&dst).expect("the loaded world captures");
    assert_eq!(again.entities, vec![lone_id]);
    assert!(again.pages.is_empty(), "{:?}", again.pages);
}

/// A record of three authored entities: two plain, one tagged.
fn three_entities() -> (SceneRecord, [PersistentId; 3]) {
    let mut src = World::new();
    let first = src.spawn((Transform::identity(), Name::new("first")));
    let second = src.spawn((Transform::identity(), Name::new("second")));
    let tagged = src.spawn((Transform::identity(), Name::new("tagged"), tag("t")));
    let ids = [first, second, tagged].map(|entity| src.mark_authored(entity).expect("authored"));
    (capture_world(&src).expect("captures"), ids)
}

/// The index of the page record holding `id`'s `component`.
fn page_index(record: &SceneRecord, id: PersistentId, component: &str) -> usize {
    slot_of(record, id, component)
        .unwrap_or_else(|| panic!("no `{component}` for {id:?}"))
        .0
}

/// A page record is a signature, rows and row-aligned columns; one whose parts
/// do not fit together cannot be built into a page, and is refused — before
/// the world is touched, so the world keeps every entity, component and
/// identity it had, and holds nothing of the record.
#[test]
fn a_malformed_page_record_is_refused_without_touching_the_world() {
    let (base, [first, second, tagged]) = three_entities();
    let plain = page_index(&base, first, "Name");
    assert_eq!(page_index(&base, second, "Name"), plain);
    let tag_value = {
        let (page, column, row) = slot_of(&base, tagged, "Tag").expect("the tag is recorded");
        base.pages[page].columns[column][row].clone()
    };

    let mut cases: Vec<(&str, SceneRecord)> = Vec::new();

    let mut record = base.clone();
    record.pages[plain].columns[0].pop();
    cases.push(("a column shorter than its rows", record));

    let mut record = base.clone();
    let rows = record.pages[plain].rows.len();
    record.pages[plain].columns[0].push(Record::Unit);
    assert_eq!(record.pages[plain].columns[0].len(), rows + 1);
    cases.push(("a column longer than its rows", record));

    let mut record = base.clone();
    let rows = record.pages[plain].rows.len();
    record.pages[plain].columns.push(vec![Record::Unit; rows]);
    cases.push(("more columns than components", record));

    let mut record = base.clone();
    record.pages[plain].components.push("Tag".into());
    cases.push(("a component without a column", record));

    let mut record = base.clone();
    let stranger = PersistentId::authored(0x5747_a6e6);
    let row = record.pages[plain]
        .rows
        .iter()
        .position(|id| *id == second)
        .expect("second's row");
    record.pages[plain].rows[row] = stranger;
    cases.push(("a row of an entity the record does not list", record));

    let mut record = base.clone();
    let page = &mut record.pages[plain];
    page.rows.push(first);
    for column in &mut page.columns {
        let value = column[0].clone();
        column.push(value);
    }
    cases.push(("an entity twice in one page", record));

    let mut record = base.clone();
    record.pages.push(PageRecord {
        components: vec!["Tag".into()],
        rows: vec![first],
        columns: vec![vec![tag_value]],
    });
    cases.push(("an entity in two pages of one domain", record));

    let mut record = base.clone();
    let name_column = {
        let page = &record.pages[plain];
        let at = page
            .components
            .iter()
            .position(|name| name == "Name")
            .expect("a name column");
        page.columns[at].clone()
    };
    record.pages[plain].components.push("Name".into());
    record.pages[plain].columns.push(name_column);
    cases.push(("one component twice in one page", record));

    let mut record = base.clone();
    record.entities.push(first);
    cases.push(("an entity listed twice", record));

    for (case, record) in cases {
        let mut world = occupied_world();
        let before = fingerprint(&world);
        let failure = apply(&mut world, &record, Identity::Keep)
            .err()
            .unwrap_or_else(|| panic!("{case}: the load must fail"));
        assert!(!failure.message.is_empty(), "{case}: the failure says why");
        assert_eq!(fingerprint(&world), before, "{case}: the world changed");
        for id in [first, second, tagged] {
            assert_eq!(
                world.entity_with_id(id),
                None,
                "{case}: {id:?} from the refused record is in the world"
            );
        }
    }
}
