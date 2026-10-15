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

//! Applying an instance's declared field to its prefab, against a project on
//! disk, as for a Rust component's: the prefab's file takes the value, the
//! override is gone from the instance, and the other instances follow.

use std::path::PathBuf;

use khora_sdk::editor_ui::{InspectedPrefab, PrefabApplyScope};
use khora_sdk::khora_core::script::{ErgonType, ScriptValue};
use khora_sdk::khora_data::ecs::{
    ComponentKey, ComponentProvenance, FieldKind, PackedField, PackedLayout, PrefabInstance,
    RuntimeComponentDecl, SemanticDomain, World,
};
use khora_sdk::khora_data::scene::{
    apply as apply_record, capture_subtree, read_scene_file, write_scene_file, CompactEncoding,
    Identity,
};
use khora_sdk::{MetricsRegistry, SceneFile};

use super::*;
use crate::ops::prefab_overrides::inspect_prefab;
use crate::project_vfs::ProjectPrefabs;

const POST: &str = "prefabs/post.kprefab";

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
    world.set_declared_by(key, Some("props/lantern.erg".to_owned()));
    key
}

/// A project on disk holding a lamp-post prefab — a root lit by a
/// `Lantern` of `glow` 2 — as `POST`, removed when the test ends.
struct Project {
    root: PathBuf,
    vfs: Arc<Mutex<ProjectVfs>>,
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn project(name: &str) -> Project {
    let mut made = World::new();
    let key = declare_lantern(&mut made);
    let root = made.spawn((Transform::identity(), Name::new("Post")));
    made.add_runtime_component(
        root,
        key,
        &ScriptValue::Struct(vec![
            ("glow".to_owned(), ScriptValue::Float(2.0)),
            ("label".to_owned(), ScriptValue::Str("post".to_owned())),
        ]),
    )
    .expect("the post's lantern attaches");
    made.mark_authored(root).expect("authored");
    let record = capture_subtree(&made, root).expect("the post captures");

    let dir = std::env::temp_dir().join(format!("khora-editor-{name}-{}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("an old project is removed");
    }
    let prefabs = dir.join("assets").join("prefabs");
    std::fs::create_dir_all(&prefabs).expect("the prefab folder");
    let file = write_scene_file(&record, &CompactEncoding).expect("the prefab writes");
    std::fs::write(prefabs.join("post.kprefab"), file.to_bytes()).expect("the prefab is written");
    let vfs =
        ProjectVfs::open(dir.clone(), Arc::new(MetricsRegistry::new())).expect("the project opens");
    Project {
        root: dir,
        vfs: Arc::new(Mutex::new(vfs)),
    }
}

impl Project {
    /// The `Lantern` of the post's root as the prefab's file holds it now.
    fn lantern_on_disk(&self) -> Vec<(String, ScriptValue)> {
        let path = self
            .root
            .join("assets")
            .join("prefabs")
            .join("post.kprefab");
        let bytes = std::fs::read(path).expect("the prefab file reads");
        let record = read_scene_file(&SceneFile::from_bytes(&bytes).expect("a scene file"))
            .expect("the prefab reads");
        let mut world = World::new();
        apply_record(&mut world, &record, Identity::Keep).expect("the prefab opens");
        let key = world
            .components()
            .key_named("Lantern")
            .expect("the prefab declares Lantern");
        let root = world
            .iter_entities()
            .find(|entity| world.declared_row(*entity, key).is_some())
            .expect("the root holds a lantern");
        world.declared_row(root, key).expect("the root's lantern")
    }

    fn inspect(&self, world: &GameWorld, entity: EntityId) -> InspectedPrefab {
        inspect_prefab(world, entity, &ProjectPrefabs(&self.vfs))
            .unwrap_or_else(|| panic!("{entity:?} belongs to no instance"))
    }
}

/// An editor world whose scripts declare `Lantern`, holding `count` posts
/// spawned from the project, by their roots.
fn posts(project: &Project, count: usize) -> (GameWorld, ComponentKey, Vec<EntityId>) {
    let mut world = GameWorld::new();
    let key = declare_lantern(world.inner_world_mut());
    let state = Arc::new(Mutex::new(EditorState::default()));
    for _ in 0..count {
        state.lock().expect("the editor state").pending_prefab_spawn =
            Some((POST.to_owned(), None));
        process_pending_prefab_spawn(Some(&project.vfs), &mut world, &state);
    }
    let roots: Vec<EntityId> = world
        .iter_entities()
        .filter(|e| world.get_component::<PrefabInstance>(*e).is_some())
        .collect();
    assert_eq!(roots.len(), count, "every post spawns");
    (world, key, roots)
}

/// Writes `entity`'s `field` of `Lantern` as a script would.
fn set(
    world: &mut GameWorld,
    key: ComponentKey,
    entity: EntityId,
    field: &str,
    value: ScriptValue,
) {
    let inner = world.inner_world_mut();
    let slot = inner
        .components()
        .vtable(key)
        .and_then(|vtable| vtable.columns.packed())
        .and_then(|layout| layout.slot_of(field))
        .unwrap_or_else(|| panic!("Lantern has `{field}`"));
    inner
        .row_mut(entity, key)
        .expect("the entity holds Lantern")
        .write_fields(&[(slot, &value)])
        .expect("the field takes the value");
}

fn field_of(
    world: &GameWorld,
    key: ComponentKey,
    entity: EntityId,
    field: &str,
) -> Option<ScriptValue> {
    world
        .inner_world()
        .declared_row(entity, key)?
        .into_iter()
        .find(|(name, _)| name == field)
        .map(|(_, value)| value)
}

/// Queues `scope` for `entity`, runs the command, and applies the edits it
/// leaves for the next frame — the other instances' rebase.
fn apply_and_rebase(
    project: &Project,
    world: &mut GameWorld,
    entity: EntityId,
    scope: PrefabApplyScope,
) {
    let state = Arc::new(Mutex::new(EditorState::default()));
    state.lock().expect("the editor state").pending_prefab_apply = Some((entity, scope));

    process_pending_prefab_apply(Some(&project.vfs), world, &state);

    let mut state = state.lock().expect("the editor state");
    crate::ops::apply_edits(world, &mut state);
}

fn glow_scope() -> PrefabApplyScope {
    PrefabApplyScope::Field {
        type_name: "Lantern".to_owned(),
        path: vec!["glow".to_owned()],
    }
}

/// **Applying a declared field writes it into the prefab's file** — and
/// nothing else the instance overrides: the override of the applied field
/// is gone, the other one stays.
#[test]
fn applying_a_declared_field_writes_it_into_the_prefab_and_clears_its_override() {
    let project = project("prefab-apply-declared-field");
    let (mut world, key, roots) = posts(&project, 1);
    let post = roots[0];
    set(&mut world, key, post, "glow", ScriptValue::Float(5.0));
    set(
        &mut world,
        key,
        post,
        "label",
        ScriptValue::Str("mine".to_owned()),
    );
    let mut marked = project.inspect(&world, post).fields_of("Lantern").to_vec();
    marked.sort();
    assert_eq!(
        marked,
        vec![vec!["glow".to_owned()], vec!["label".to_owned()]],
        "both fields override the prefab"
    );

    apply_and_rebase(&project, &mut world, post, glow_scope());

    assert_eq!(
        project.lantern_on_disk(),
        vec![
            ("glow".to_owned(), ScriptValue::Float(5.0)),
            ("label".to_owned(), ScriptValue::Str("post".to_owned())),
        ],
        "the prefab takes the glow, not the label"
    );
    assert_eq!(
        project.inspect(&world, post).fields_of("Lantern"),
        &[vec!["label".to_owned()]][..],
        "the glow is no override any more; the label still is"
    );
}

/// **The other instances follow an applied declared field** at once — and
/// keep what they override themselves.
#[test]
fn another_live_instance_takes_an_applied_declared_field() {
    let project = project("prefab-apply-declared-rebase");
    let (mut world, key, roots) = posts(&project, 2);
    let (post, other) = (roots[0], roots[1]);
    set(&mut world, key, post, "glow", ScriptValue::Float(5.0));
    set(
        &mut world,
        key,
        other,
        "label",
        ScriptValue::Str("own".to_owned()),
    );

    apply_and_rebase(&project, &mut world, post, glow_scope());

    assert_eq!(
        field_of(&world, key, other, "glow"),
        Some(ScriptValue::Float(5.0)),
        "the other post takes the applied glow"
    );
    assert_eq!(
        field_of(&world, key, other, "label"),
        Some(ScriptValue::Str("own".to_owned())),
        "and keeps its own label"
    );
    assert_eq!(
        project.inspect(&world, other).fields_of("Lantern"),
        &[vec!["label".to_owned()]][..]
    );
}

/// Registers `Spark { int sparks = 1; }`, as `props/spark.erg` declares it.
fn declare_spark(world: &mut World) -> ComponentKey {
    let key = world
        .register_runtime_component(RuntimeComponentDecl {
            name: "Spark".to_owned(),
            domain: SemanticDomain::Script,
            provenance: ComponentProvenance::Authored,
            resumable: false,
            layout: PackedLayout::typed(
                vec![PackedField {
                    name: "sparks".to_owned(),
                    kind: FieldKind::Int,
                    default: ScriptValue::Int(1),
                }],
                vec![ErgonType::Int],
            )
            .expect("a valid layout"),
        })
        .expect("Spark registers");
    world.set_declared_by(key, Some("props/spark.erg".to_owned()));
    key
}

/// **A declared component applied to the prefab reaches the other
/// instances** at once, with the applied values — the prefab gained it, so
/// every instance that had it not gains it too.
#[test]
fn another_live_instance_gains_an_applied_declared_component() {
    let project = project("prefab-apply-declared-gained");
    let (mut world, _, roots) = posts(&project, 2);
    let (post, other) = (roots[0], roots[1]);
    let spark = declare_spark(world.inner_world_mut());
    world
        .inner_world_mut()
        .add_runtime_component(
            post,
            spark,
            &ScriptValue::Struct(vec![("sparks".to_owned(), ScriptValue::Int(4))]),
        )
        .expect("a spark attaches to the post");

    apply_and_rebase(
        &project,
        &mut world,
        post,
        PrefabApplyScope::Component {
            type_name: "Spark".to_owned(),
        },
    );

    assert_eq!(
        world.inner_world().declared_row(other, spark),
        Some(vec![("sparks".to_owned(), ScriptValue::Int(4))]),
        "the other post gains the applied spark"
    );
    assert_eq!(project.inspect(&world, other).override_count(), 0);
}

/// **A declared component the prefab lost leaves the other instances** that
/// did not make it their own — and stays on one that did.
#[test]
fn a_declared_component_removed_from_the_prefab_leaves_the_untouched_instances() {
    let project = project("prefab-apply-declared-lost");
    let (mut world, key, roots) = posts(&project, 3);
    let (post, untouched, owned) = (roots[0], roots[1], roots[2]);
    set(&mut world, key, owned, "glow", ScriptValue::Float(9.0));
    assert!(world.inner_world_mut().remove_component_by_key(post, key));

    apply_and_rebase(&project, &mut world, post, PrefabApplyScope::Instance);

    assert_eq!(
        world.inner_world().declared_row(untouched, key),
        None,
        "the untouched post loses its lantern with the prefab"
    );
    assert_eq!(
        field_of(&world, key, owned, "glow"),
        Some(ScriptValue::Float(9.0)),
        "the post that set its own glow keeps its lantern"
    );
}
