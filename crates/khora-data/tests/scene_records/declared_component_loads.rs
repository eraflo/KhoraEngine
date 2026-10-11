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

//! A scene's declared components meeting a world: mapped by field name onto
//! the world's own declaration, which wins; refused whole when the file
//! claims a Rust component's name or names a component nobody knows; and a
//! failed or abandoned load registers nothing. A scene holding none is
//! written exactly as it always was.

use khora_core::math::Vec3;
use khora_core::script::ScriptValue;
use khora_data::ecs::{FieldKind, Name, SemanticDomain, Transform};
use khora_data::scene::prepare;
use khora_data::scene::record::ReportKind;

use super::declared_components::{
    declare, declared_fields, declared_world, domain_of, field, field_of, health_fields,
    mood_fields, translations, NO_ONE,
};
use super::*;

/// `Health` as a later version of the scripts declares it, in `Script`:
/// `current` became a `float`, `max` a `bool`, `label` is gone and
/// `shield` is new.
fn health_since() -> Vec<khora_data::ecs::PackedField> {
    vec![
        field("current", FieldKind::Float, ScriptValue::Float(0.0)),
        field("max", FieldKind::Bool, ScriptValue::Bool(false)),
        field("shield", FieldKind::Int, ScriptValue::Int(7)),
        field("target", FieldKind::Entity, ScriptValue::Entity(NO_ONE)),
        field("ally", FieldKind::Value, ScriptValue::Null),
    ]
}

/// Whether `entries` hold one of `kind` for `component`'s field `path`.
fn reports(
    entries: &[khora_data::scene::record::ReportEntry],
    component: &str,
    path: &str,
    kind: &ReportKind,
) -> bool {
    entries.iter().any(|entry| {
        entry.component.as_deref() == Some(component) && entry.path == path && entry.kind == *kind
    })
}

/// A world already holding two plain entities, the state a failed load must
/// leave it in.
fn open_world() -> World {
    let mut world = World::new();
    for x in [10.0, 20.0] {
        let entity = world.spawn(Transform::from_translation(Vec3::new(x, 0.0, 0.0)));
        world.mark_authored(entity).expect("authored");
    }
    world
}

/// Asserts `world` is still `open_world()`: its two entities, and no
/// declared component registered.
fn assert_untouched(world: &World, case: &str) {
    assert_eq!(world.iter_entities().count(), 2, "{case}: entity count");
    assert_eq!(translations(world), vec![10.0, 20.0], "{case}");
    for component in ["Health", "Mood"] {
        assert_eq!(
            world.components().key_named(component),
            None,
            "{case}: `{component}` was registered by a load that did not happen"
        );
    }
}

/// A precondition of the refusal tests: the record they damage loads whole
/// into a fresh world and registers what it declares.
fn assert_loads(record: &SceneRecord) {
    let mut world = World::new();
    apply(&mut world, record, Identity::Keep)
        .unwrap_or_else(|e| panic!("the undamaged record loads: {e}"));
    for component in ["Health", "Mood"] {
        assert!(
            world.components().key_named(component).is_some(),
            "the undamaged record registers `{component}`"
        );
    }
}

/// The world's own declaration is the truth: a field the file lacks takes
/// the world's default (`Defaulted`), one the world no longer declares is
/// dropped (`Dropped`), one whose kind changed is dropped and takes the
/// default, an `int` that became a `float` is kept, converted (`Widened`) —
/// and the rows land in the world's domain, not the file's. The world's
/// declaration is left as it was.
#[test]
fn a_scene_maps_declared_fields_by_name_onto_the_live_declaration() {
    let src = declared_world();
    let record = capture_world(&src.world).expect("captures");
    for (name, encoding) in every_encoding() {
        let back = through(&record, name, encoding);
        let mut dst = World::new();
        declare(&mut dst, "Health", SemanticDomain::Script, health_since());
        declare(&mut dst, "Mood", SemanticDomain::Script, mood_fields());
        let applied = apply(&mut dst, &back, Identity::Keep)
            .unwrap_or_else(|e| panic!("{name}: the load failed: {e}"));
        let map = entity_map(&src.world, &dst);
        let (a, c) = (map[&src.a], map[&src.c]);

        assert_eq!(
            field_of(&dst, a, "Health", "current"),
            Some(ScriptValue::Float(40.0)),
            "{name}: an int read into a float field is kept, converted"
        );
        assert_eq!(
            field_of(&dst, c, "Health", "current"),
            Some(ScriptValue::Float(100.0)),
            "{name}"
        );
        assert_eq!(
            field_of(&dst, a, "Health", "max"),
            Some(ScriptValue::Bool(false)),
            "{name}: a field of another kind takes the world's default"
        );
        assert_eq!(
            field_of(&dst, a, "Health", "shield"),
            Some(ScriptValue::Int(7)),
            "{name}: a field the file lacks takes the world's default"
        );
        assert_eq!(
            field_of(&dst, a, "Health", "target"),
            Some(ScriptValue::Entity(c)),
            "{name}: the fields both know keep their values"
        );
        assert_eq!(
            field_of(&dst, a, "Mood", "n"),
            Some(ScriptValue::Int(7)),
            "{name}: `Mood` beside it, in the same domain now, is whole"
        );

        let entries = &applied.report.entries;
        for (path, kind) in [
            ("current", ReportKind::Widened),
            ("max", ReportKind::Dropped),
            ("label", ReportKind::Dropped),
            ("shield", ReportKind::Defaulted),
        ] {
            assert!(
                reports(entries, "Health", path, &kind),
                "{name}: `Health.{path}` is reported {kind:?}: {entries:?}"
            );
        }

        assert_eq!(
            domain_of(&dst, "Health"),
            Some(SemanticDomain::Script),
            "{name}: the world's domain wins over the file's"
        );
        assert_eq!(
            declared_fields(&dst, "Health")
                .map(|fields| fields.iter().map(|f| f.name.clone()).collect::<Vec<_>>()),
            Some(
                health_since()
                    .iter()
                    .map(|f| f.name.clone())
                    .collect::<Vec<_>>()
            ),
            "{name}: a load never relays out the world's declaration"
        );
    }
}

/// A file that declares a component under the name of a Rust component
/// fails the whole load, naming it, and leaves the world as it was.
#[test]
fn a_file_declaration_named_like_a_rust_component_fails_the_load() {
    let src = declared_world();
    let record = capture_world(&src.world).expect("captures");
    let bytes = TextEncoding.encode(&record).expect("encodes");
    let mut json: serde_json::Value = serde_json::from_slice(&bytes).expect("the text is JSON");
    let top = json.as_object_mut().expect("a scene is an object");

    // No page holds `Mood` any more: what is left of it is the file's own
    // description of it, renamed `Name`.
    let pages = top
        .get_mut("pages")
        .and_then(serde_json::Value::as_array_mut)
        .expect("pages");
    for page in pages.iter_mut() {
        if let Some(columns) = page
            .get_mut("columns")
            .and_then(serde_json::Value::as_object_mut)
        {
            columns.remove("Mood");
        }
    }
    pages.retain(|page| {
        page.get("columns")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|columns| !columns.is_empty())
    });
    let mut renamed = 0;
    for (key, value) in top.iter_mut() {
        if !matches!(key.as_str(), "entities" | "pages" | "instances") {
            renamed += rename_strings(value, "Mood", "Name");
        }
    }
    assert!(renamed > 0, "the file describes `Mood` beside its pages");
    let damaged = TextEncoding
        .decode(&serde_json::to_vec(&json).expect("JSON writes"))
        .expect("the renamed file is well formed");

    let mut world = open_world();
    match apply(&mut world, &damaged, Identity::Keep) {
        Ok(_) => panic!("a declaration named like a Rust component was accepted"),
        Err(failure) => assert!(
            failure.message.contains("Name"),
            "the refusal names the component: {}",
            failure.message
        ),
    }
    assert_untouched(&world, "a Rust name declared");
    assert!(world.query::<&Name>().next().is_none());
}

/// Every string, and every object key, of `value` equal to `from` made `to`
/// — how many.
fn rename_strings(value: &mut serde_json::Value, from: &str, to: &str) -> usize {
    match value {
        serde_json::Value::String(text) if text == from => {
            *text = to.to_owned();
            1
        }
        serde_json::Value::Array(items) => items
            .iter_mut()
            .map(|item| rename_strings(item, from, to))
            .sum(),
        serde_json::Value::Object(fields) => {
            let mut renamed = 0;
            if let Some(inner) = fields.remove(from) {
                fields.insert(to.to_owned(), inner);
                renamed += 1;
            }
            renamed
                + fields
                    .values_mut()
                    .map(|item| rename_strings(item, from, to))
                    .sum::<usize>()
        }
        _ => 0,
    }
}

/// A component name that is neither a Rust component's, nor in the file's
/// own table, nor retired still fails the whole load — a declaration table
/// beside it changes nothing — and registers none of what the table holds.
#[test]
fn an_undeclared_unknown_component_still_fails_the_load() {
    let src = declared_world();
    let record = capture_world(&src.world).expect("captures");
    assert_loads(&record);

    let mut ghost = record.clone();
    add_column(&mut ghost, 0, "Ghost", Record::Unit);
    let mut world = open_world();
    let failure = apply(&mut world, &ghost, Identity::Keep)
        .expect_err("an unknown component name fails the load");
    assert!(failure.message.contains("Ghost"), "{}", failure.message);
    assert_untouched(&world, "an unknown name");

    // `Mood`'s column under a name the table does not describe.
    let mut misnamed = record.clone();
    assert!(rename_component(&mut misnamed, "Mood", "Moody") > 0);
    let failure = apply(&mut world, &misnamed, Identity::Keep)
        .expect_err("a column the table does not describe fails the load");
    assert!(failure.message.contains("Moody"), "{}", failure.message);
    assert_untouched(&world, "a column named unlike its declaration");
}

/// A load that fails once its names are read — on a value — registers none
/// of the declarations the file holds.
#[test]
fn a_load_failing_on_a_value_registers_no_declaration() {
    let src = declared_world();
    let record = capture_world(&src.world).expect("captures");
    assert_loads(&record);

    let mut damaged = record.clone();
    let id = src.world.persistent_id(src.d).expect("an id");
    *value_mut(&mut damaged, id, "Transform") = Record::Str("not a transform".into());
    let mut world = open_world();
    assert!(
        apply(&mut world, &damaged, Identity::Keep).is_err(),
        "a damaged value fails the load"
    );
    assert_untouched(&world, "a damaged value");
}

/// A prepared load registers nothing until it is committed: abandoned, it
/// leaves the registry as it was; committed, the declarations are there.
#[test]
fn only_a_committed_load_registers_its_declarations() {
    let src = declared_world();
    let record = capture_world(&src.world).expect("captures");
    let mut world = open_world();

    let prepared = prepare(&mut world, &record).expect("the record is read");
    assert_untouched(&world, "prepared, not committed");
    prepared.abandon(&mut world);
    assert_untouched(&world, "abandoned");

    prepare(&mut world, &record)
        .expect("the record is read again")
        .commit(&mut world, Identity::Keep);
    assert_eq!(
        declared_fields(&world, "Health").map(|fields| fields.len()),
        Some(health_fields().len()),
        "the committed load registered `Health`"
    );
    assert_eq!(
        declared_fields(&world, "Mood").map(|fields| fields.len()),
        Some(mood_fields().len()),
        "the committed load registered `Mood`"
    );
    assert_eq!(world.iter_entities().count(), 6);
}

/// What `CompactEncoding` wrote for a two-entity scene — a parent named
/// `Alpha` at (1, 2, 3) and its child `Beta` at (-1, 0, 0.5) — before
/// declared components were saved: compact layout version 2.
const COMPACT_V2_SCENE: &[u8] = &[
    0x02, 0x0f, 0x04, 0x4e, 0x61, 0x6d, 0x65, 0x06, 0x50, 0x61, 0x72, 0x65, 0x6e, 0x74, 0x09, 0x54,
    0x72, 0x61, 0x6e, 0x73, 0x66, 0x6f, 0x72, 0x6d, 0x10, 0x53, 0x65, 0x72, 0x69, 0x61, 0x6c, 0x69,
    0x7a, 0x61, 0x62, 0x6c, 0x65, 0x4e, 0x61, 0x6d, 0x65, 0x12, 0x53, 0x65, 0x72, 0x69, 0x61, 0x6c,
    0x69, 0x7a, 0x61, 0x62, 0x6c, 0x65, 0x50, 0x61, 0x72, 0x65, 0x6e, 0x74, 0x15, 0x53, 0x65, 0x72,
    0x69, 0x61, 0x6c, 0x69, 0x7a, 0x61, 0x62, 0x6c, 0x65, 0x54, 0x72, 0x61, 0x6e, 0x73, 0x66, 0x6f,
    0x72, 0x6d, 0x0b, 0x74, 0x72, 0x61, 0x6e, 0x73, 0x6c, 0x61, 0x74, 0x69, 0x6f, 0x6e, 0x08, 0x72,
    0x6f, 0x74, 0x61, 0x74, 0x69, 0x6f, 0x6e, 0x05, 0x73, 0x63, 0x61, 0x6c, 0x65, 0x04, 0x56, 0x65,
    0x63, 0x33, 0x01, 0x78, 0x01, 0x79, 0x01, 0x7a, 0x0a, 0x51, 0x75, 0x61, 0x74, 0x65, 0x72, 0x6e,
    0x69, 0x6f, 0x6e, 0x01, 0x77, 0x03, 0x00, 0x05, 0x03, 0x06, 0x07, 0x08, 0x00, 0x09, 0x03, 0x0a,
    0x0b, 0x0c, 0x00, 0x0d, 0x04, 0x0a, 0x0b, 0x0c, 0x0e, 0x02, 0x93, 0x40, 0xc4, 0x14, 0x42, 0xc3,
    0x72, 0x78, 0x27, 0x4b, 0xdd, 0xd4, 0x68, 0x0c, 0x57, 0x07, 0x02, 0x03, 0x00, 0x01, 0x02, 0x01,
    0x27, 0x4b, 0xdd, 0xd4, 0x68, 0x0c, 0x57, 0x07, 0x11, 0x03, 0x08, 0x04, 0x42, 0x65, 0x74, 0x61,
    0x11, 0x04, 0x13, 0x93, 0x40, 0xc4, 0x14, 0x42, 0xc3, 0x72, 0x78, 0x0f, 0x00, 0x0f, 0x01, 0x05,
    0x00, 0x00, 0x80, 0xbf, 0x05, 0x00, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x3f, 0x0f, 0x02,
    0x05, 0x00, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x05,
    0x00, 0x00, 0x80, 0x3f, 0x0f, 0x01, 0x05, 0x00, 0x00, 0x80, 0x3f, 0x05, 0x00, 0x00, 0x80, 0x3f,
    0x05, 0x00, 0x00, 0x80, 0x3f, 0x02, 0x00, 0x02, 0x01, 0x93, 0x40, 0xc4, 0x14, 0x42, 0xc3, 0x72,
    0x78, 0x11, 0x03, 0x08, 0x05, 0x41, 0x6c, 0x70, 0x68, 0x61, 0x0f, 0x00, 0x0f, 0x01, 0x05, 0x00,
    0x00, 0x80, 0x3f, 0x05, 0x00, 0x00, 0x00, 0x40, 0x05, 0x00, 0x00, 0x40, 0x40, 0x0f, 0x02, 0x05,
    0x00, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x05, 0x00,
    0x00, 0x80, 0x3f, 0x0f, 0x01, 0x05, 0x00, 0x00, 0x80, 0x3f, 0x05, 0x00, 0x00, 0x80, 0x3f, 0x05,
    0x00, 0x00, 0x80, 0x3f, 0x00,
];

/// A compact file of layout version 2 still loads, and a scene holding no
/// declared component is written byte for byte as version 2 wrote it — in
/// text, with no key beyond its entities and pages. Only a scene holding
/// declared components is written in the new layout.
#[test]
fn only_a_scene_holding_declared_components_changes_its_layout() {
    let record = CompactEncoding
        .decode(COMPACT_V2_SCENE)
        .expect("a version 2 compact scene reads");
    let mut world = World::new();
    apply(&mut world, &record, Identity::Keep).expect("a version 2 compact scene loads");
    let mut names: Vec<(String, Vec3)> = world
        .query::<(&Name, &Transform)>()
        .map(|(name, t)| (name.as_str().to_owned(), t.translation))
        .collect();
    names.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        names,
        vec![
            ("Alpha".to_owned(), Vec3::new(1.0, 2.0, 3.0)),
            ("Beta".to_owned(), Vec3::new(-1.0, 0.0, 0.5)),
        ]
    );
    assert_eq!(
        CompactEncoding.encode(&record).expect("encodes"),
        COMPACT_V2_SCENE,
        "a scene without declared components is written as before"
    );

    let plain = capture_world(&world).expect("captures");
    let text: serde_json::Value =
        serde_json::from_slice(&TextEncoding.encode(&plain).expect("encodes")).expect("JSON");
    let keys: Vec<&str> = text
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, vec!["entities", "pages"]);

    let declared = capture_world(&declared_world().world).expect("captures");
    let compact = CompactEncoding.encode(&declared).expect("encodes");
    assert_eq!(
        compact.first(),
        Some(&3),
        "a scene holding declared components is compact layout version 3"
    );
    let text: serde_json::Value =
        serde_json::from_slice(&TextEncoding.encode(&declared).expect("encodes")).expect("JSON");
    assert!(
        text.as_object().expect("an object").len() > 2,
        "a text scene holding declared components describes them beside its pages: {text}"
    );
    let mut fresh = World::new();
    apply(
        &mut fresh,
        &CompactEncoding.decode(&compact).expect("version 3 reads"),
        Identity::Keep,
    )
    .expect("version 3 loads");
    assert!(fresh.components().key_named("Health").is_some());
}

/// A one-entity scene whose `Health` is declared with `fields`, its fields
/// set from `value`.
fn scene_declaring(fields: Vec<khora_data::ecs::PackedField>, value: ScriptValue) -> SceneRecord {
    let mut world = World::new();
    declare(&mut world, "Health", SemanticDomain::Physics, fields);
    let entity = world.spawn(Transform::identity());
    super::declared_components::attach(&mut world, entity, "Health", value);
    world.mark_authored(entity).expect("authored");
    capture_world(&world).expect("captures")
}

/// Two loads prepared into a world that declares nothing, from files that
/// declare `Health` differently, then both committed: the first registers
/// its declaration; the second's rows must still land in the fields they
/// were read for — by name — not in whatever field now holds their slot.
#[test]
fn a_load_committed_after_another_registered_its_component_keeps_its_fields() {
    let first = scene_declaring(
        vec![
            field("current", FieldKind::Int, ScriptValue::Int(100)),
            field("max", FieldKind::Int, ScriptValue::Int(100)),
        ],
        ScriptValue::Struct(vec![
            ("current".into(), ScriptValue::Int(1)),
            ("max".into(), ScriptValue::Int(2)),
        ]),
    );
    // The same fields, declared the other way round.
    let second = scene_declaring(
        vec![
            field("max", FieldKind::Int, ScriptValue::Int(100)),
            field("current", FieldKind::Int, ScriptValue::Int(100)),
        ],
        ScriptValue::Struct(vec![
            ("current".into(), ScriptValue::Int(30)),
            ("max".into(), ScriptValue::Int(40)),
        ]),
    );
    let mut world = World::new();
    let one = prepare(&mut world, &first).expect("the first file is read");
    let two = prepare(&mut world, &second).expect("the second file is read");
    one.commit(&mut world, Identity::Fresh);
    let applied = two.commit(&mut world, Identity::Fresh);
    let (_, entity) = applied.entities[0];
    assert_eq!(
        (
            field_of(&world, entity, "Health", "current"),
            field_of(&world, entity, "Health", "max"),
        ),
        (Some(ScriptValue::Int(30)), Some(ScriptValue::Int(40))),
        "the second load's values are in the fields they were saved in"
    );
}

/// A declared component this world keeps in another domain than the file
/// did reaches an entity that holds nothing else: the entity is loaded,
/// with its values, in the world's domain.
#[test]
fn a_moved_declared_component_reaches_an_entity_holding_nothing_else() {
    let mut src = World::new();
    declare(
        &mut src,
        "Health",
        SemanticDomain::Physics,
        super::declared_components::health_fields(),
    );
    let entity = src.spawn(());
    super::declared_components::attach(
        &mut src,
        entity,
        "Health",
        ScriptValue::Struct(vec![("current".into(), ScriptValue::Int(12))]),
    );
    src.mark_authored(entity).expect("authored");
    let id = src.persistent_id(entity).expect("an id");
    let record = capture_world(&src).expect("captures");
    assert!(
        record
            .pages
            .iter()
            .any(|page| page.components == vec!["Health".to_owned()]),
        "the entity holds `Health` alone"
    );
    for identity in [Identity::Keep, Identity::Fresh] {
        let mut dst = World::new();
        declare(
            &mut dst,
            "Health",
            SemanticDomain::Script,
            super::declared_components::health_fields(),
        );
        let applied = apply(&mut dst, &record, identity)
            .unwrap_or_else(|e| panic!("{identity:?}: the load failed: {e}"));
        let (_, twin) = applied.entities[0];
        if identity == Identity::Keep {
            assert_eq!(dst.entity_with_id(id), Some(twin));
        }
        assert_eq!(
            field_of(&dst, twin, "Health", "current"),
            Some(ScriptValue::Int(12)),
            "{identity:?}"
        );
        assert_eq!(dst.iter_entities().count(), 1, "{identity:?}");
    }
}

/// A file that declares one component twice contradicts itself — which
/// declaration its cells were written under cannot be told — and fails the
/// whole load, naming it, the way a page naming one component twice does.
#[test]
fn a_file_declaring_a_component_twice_fails_the_load() {
    let src = declared_world();
    let mut record = capture_world(&src.world).expect("captures");
    assert_loads(&record);
    let mut again = record
        .declared
        .iter()
        .find(|declared| declared.name == "Health")
        .expect("the file declares `Health`")
        .clone();
    // The same name, its `current` a float now.
    again.fields[0].kind = FieldKind::Float;
    again.fields[0].default = Record::F32(0.0);
    record.declared.push(again);
    for (name, encoding) in every_encoding() {
        let back = through(&record, name, encoding);
        let mut world = open_world();
        let failure = apply(&mut world, &back, Identity::Keep)
            .err()
            .unwrap_or_else(|| panic!("{name}: a file declaring `Health` twice was accepted"));
        assert!(
            failure.message.contains("Health"),
            "{name}: {}",
            failure.message
        );
        assert_untouched(&world, "a component declared twice");
    }
}

/// A compact scene holding declared components (layout version 3), damaged
/// anywhere — cut short, a byte set to 0xFF — is refused, or read and
/// loaded or refused by the load, never a panic.
#[test]
fn a_damaged_compact_scene_of_declared_components_never_panics() {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    let src = declared_world();
    let record = capture_world(&src.world).expect("captures");
    let bytes = CompactEncoding.encode(&record).expect("encodes");
    assert_eq!(bytes.first(), Some(&3));
    let n = bytes.len();
    let attempt = |input: &[u8], case: String| {
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            if let Ok(back) = CompactEncoding.decode(input) {
                let mut world = World::new();
                let _ = apply(&mut world, &back, Identity::Keep);
            }
        }));
        assert!(outcome.is_ok(), "{case} panicked");
    };
    for cut in 0..n {
        let prefix = &bytes[..cut];
        assert!(
            CompactEncoding.decode(prefix).is_err(),
            "a file cut at {cut} of {n} bytes was accepted"
        );
    }
    for at in 0..n {
        let mut flipped = bytes.clone();
        flipped[at] = 0xFF;
        attempt(&flipped, format!("byte {at} of {n} set to 0xFF"));
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(
        CompactEncoding.decode(&trailing).is_err(),
        "a trailing byte was accepted"
    );
}

/// An `int` read into what is now a `float` field is kept only where it
/// converts losslessly; one that does not (2^24 + 1 has no `f32`) takes the
/// default — and the report says so for that entity: its value was
/// dropped, not widened.
#[test]
fn an_int_too_wide_for_a_float_field_is_reported_dropped() {
    let record = scene_declaring(
        super::declared_components::health_fields(),
        ScriptValue::Struct(vec![("current".into(), ScriptValue::Int(16_777_217))]),
    );
    let mut world = World::new();
    declare(
        &mut world,
        "Health",
        SemanticDomain::Physics,
        health_since(),
    );
    let applied = apply(&mut world, &record, Identity::Keep).expect("the scene loads");
    let (id, entity) = applied.entities[0];
    let current = field_of(&world, entity, "Health", "current");
    let entries: Vec<_> = applied
        .report
        .entries
        .iter()
        .filter(|entry| entry.entity == Some(id) && entry.path == "current")
        .map(|entry| entry.kind.clone())
        .collect();
    assert_eq!(
        current,
        Some(ScriptValue::Float(0.0)),
        "the field holds its default"
    );
    assert_eq!(
        entries,
        vec![ReportKind::Dropped],
        "the report says the value was dropped"
    );
}
