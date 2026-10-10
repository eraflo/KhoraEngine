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

//! A script writes the engine's components, end to end.
//!
//! Compiled against the mirrors the engine really serves
//! (`import "engine/components.erg"`, answered by `PreludeLoader`), run on the
//! VM, and what it queued applied to a real `World` by the applier the
//! `Maintenance` phase runs. Each test reads the value back **and** counts
//! the commands applied: a write that was dropped and one that never left the
//! script look the same from the value alone.

use khora_core::ecs::entity::EntityId;
use khora_core::math::{Quaternion, Vec3};
use khora_core::physics::BodyType;
use khora_data::ecs::systems::script_commands::apply_all;
use khora_data::ecs::{Collider, RigidBody, Transform, World};
use khora_io::script::mirror::PreludeLoader;
use khora_script::vm::{Machine, Program, Run};
use khora_script::{compile_module, CompileOutcome, Diagnostic, Host, MemoryLoader};

/// `main.erg`, importing the engine's mirrors, compiled.
fn compile(main: &str) -> CompileOutcome {
    let source = format!("import \"engine/components.erg\";\n{main}");
    let loader = PreludeLoader::new(MemoryLoader::new().with("main.erg", source));
    compile_module(&loader, "main.erg")
}

fn build(main: &str) -> Program {
    let result = compile(main);
    let errors: Vec<&Diagnostic> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "should compile: {errors:?}");
    result.program.expect("a program with no errors")
}

/// Runs `member` as `entity`'s behavior, then applies what it queued to
/// `world`. Returns how many commands were applied.
fn run_and_apply(world: &mut World, main: &str, member: &str, entity: EntityId) -> usize {
    let program = build(main);
    let mut host = Host::new().for_entity(entity);
    let mut machine = Machine::new(&program, member, &[]).expect("the member exists");
    assert_eq!(machine.run(&program, &mut host, u64::MAX), Run::Completed);
    let queued = host.end_frame();
    assert!(!queued.is_empty(), "the script queued its write");
    apply_all(world, &queued)
}

fn said(diagnostic: &Diagnostic) -> String {
    format!(
        "{} | {}",
        diagnostic.message,
        diagnostic.note.as_deref().unwrap_or_default()
    )
}

/// The refusal of `main` saying `fragment`, after proving the mirrors compile
/// on their own — so the refusal is about `main`.
fn refused_saying(main: &str, fragment: &str) -> Diagnostic {
    let control = compile("fn void Probe() { }");
    assert!(
        control.succeeded(),
        "the mirrors compile: {:?}",
        control.diagnostics
    );
    let result = compile(main);
    assert!(
        !result.succeeded(),
        "should be refused, and compiled:\n{main}"
    );
    result
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .find(|d| said(d).contains(fragment))
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "expected a refusal saying {fragment:?}; got {:?}",
                result.diagnostics.iter().map(said).collect::<Vec<_>>()
            )
        })
}

/// An authored rigid body, every field away from its default so a field the
/// write should not touch is seen to keep its value.
fn authored_body() -> RigidBody {
    RigidBody {
        body_type: BodyType::Kinematic,
        mass: 2.0,
        ccd_enabled: true,
        initial_velocity: Vec3::new(1.0, 2.0, 3.0),
        initial_angular_velocity: Vec3::new(0.0, 4.0, 0.0),
        ..RigidBody::default()
    }
}

fn collider(friction: f32) -> Collider {
    Collider {
        friction,
        restitution: 0.25,
        is_sensor: true,
        ..Collider::default()
    }
}

const ANCHOR_SET: &str = "behavior Anchor {
    void OnSpawn() { this.Set(RigidBody { mass: 12.0 }); }
}";

// ─── Set ────────────────────────────────────────────────────────────────────

#[test]
fn set_changes_the_named_field() {
    let mut world = World::new();
    let entity = world.spawn((Transform::identity(), authored_body()));

    assert_eq!(
        run_and_apply(&mut world, ANCHOR_SET, "Anchor.OnSpawn", entity),
        1
    );

    let body = world.get::<RigidBody>(entity).expect("still a rigid body");
    assert_eq!(body.mass, 12.0);
}

/// **A patch.** `ccd_enabled`, the velocities and the body type — the last
/// one a field the mirror cannot even name — keep their values.
#[test]
fn set_leaves_unnamed_fields_alone() {
    let mut world = World::new();
    let entity = world.spawn((Transform::identity(), authored_body()));

    assert_eq!(
        run_and_apply(&mut world, ANCHOR_SET, "Anchor.OnSpawn", entity),
        1
    );

    let body = world.get::<RigidBody>(entity).expect("still a rigid body");
    let authored = authored_body();
    assert_eq!(body.body_type, authored.body_type);
    assert_eq!(body.ccd_enabled, authored.ccd_enabled);
    assert_eq!(body.initial_velocity, authored.initial_velocity);
    assert_eq!(
        body.initial_angular_velocity,
        authored.initial_angular_velocity
    );
}

// ─── Add and Remove ─────────────────────────────────────────────────────────

const ANCHOR_ADD: &str = "behavior Anchor {
    void OnSpawn() { this.Add(Collider { friction: 0.8 }); }
}";

/// `Add` attaches the component's default, then the fields written over it.
#[test]
fn add_attaches_with_defaults_and_patch() {
    let mut world = World::new();
    let entity = world.spawn(Transform::identity());

    assert_eq!(
        run_and_apply(&mut world, ANCHOR_ADD, "Anchor.OnSpawn", entity),
        1
    );

    let attached = world
        .get::<Collider>(entity)
        .expect("a collider was attached");
    let default = Collider::default();
    assert_eq!(attached.friction, 0.8);
    assert_eq!(attached.restitution, default.restitution);
    assert_eq!(attached.is_sensor, default.is_sensor);
}

/// Adding what is already there would reset it to its defaults, which is
/// never what "add" meant: the applier refuses (`AlreadyAttached`, logged),
/// nothing is applied, and the component keeps every field.
#[test]
fn add_to_an_entity_that_has_it_is_refused() {
    let mut world = World::new();
    let entity = world.spawn((Transform::identity(), collider(0.3)));

    assert_eq!(
        run_and_apply(&mut world, ANCHOR_ADD, "Anchor.OnSpawn", entity),
        0
    );

    let kept = world
        .get::<Collider>(entity)
        .expect("the collider is still there");
    assert_eq!(kept.friction, 0.3, "not overwritten by the patch");
    assert_eq!(kept.restitution, 0.25, "not reset to its default");
    assert!(kept.is_sensor);
}

#[test]
fn remove_detaches() {
    let mut world = World::new();
    let entity = world.spawn((Transform::identity(), collider(0.3), authored_body()));

    let applied = run_and_apply(
        &mut world,
        "behavior Anchor {
             void Release() { this.Remove(Collider); }
         }",
        "Anchor.Release",
        entity,
    );

    assert_eq!(applied, 1);
    assert!(
        world.get::<Collider>(entity).is_none(),
        "the collider is gone"
    );
    assert!(
        world.get::<RigidBody>(entity).is_some(),
        "and nothing else went with it"
    );
}

// ─── Spawn ──────────────────────────────────────────────────────────────────

/// `Spawn` places a new entity at the position, with the identity rotation,
/// carrying each component — its defaults with the fields written over them.
#[test]
fn spawn_creates_an_entity_with_its_components() {
    let mut world = World::new();
    let spawner = world.spawn(Transform::identity());

    let applied = run_and_apply(
        &mut world,
        "behavior Spawner {
             void OnSpawn() {
                 Spawn(Vec3(1.0, 2.0, 3.0), RigidBody { mass: 3.0 }, Collider { friction: 0.8 });
             }
         }",
        "Spawner.OnSpawn",
        spawner,
    );

    assert_eq!(applied, 1);
    let spawned: Vec<EntityId> = world.iter_entities().filter(|e| *e != spawner).collect();
    assert_eq!(spawned.len(), 1, "one entity was spawned");
    let entity = spawned[0];

    let transform = world.get::<Transform>(entity).expect("placed");
    assert_eq!(transform.translation, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(transform.rotation, Quaternion::IDENTITY);

    let body = world.get::<RigidBody>(entity).expect("with its rigid body");
    assert_eq!(body.mass, 3.0);
    assert_eq!(body.ccd_enabled, RigidBody::default().ccd_enabled);

    let attached = world.get::<Collider>(entity).expect("and its collider");
    assert_eq!(attached.friction, 0.8);
    assert_eq!(attached.restitution, Collider::default().restitution);
}

/// **A half-built entity is not left behind.** The second component refuses
/// its value — a NaN, which no component stores — so the spawn is rolled back
/// whole, the collider attached before it included.
#[test]
fn spawn_rolls_back_when_a_component_refuses() {
    let mut world = World::new();
    let spawner = world.spawn(Transform::identity());

    let applied = run_and_apply(
        &mut world,
        "behavior Spawner {
             void OnSpawn() {
                 float zero = 0.0;
                 Spawn(Vec3(0.0, 0.0, 0.0), Collider { friction: 0.8 }, RigidBody { mass: zero / zero });
             }
         }",
        "Spawner.OnSpawn",
        spawner,
    );

    assert_eq!(applied, 0);
    assert_eq!(
        world.iter_entities().collect::<Vec<_>>(),
        vec![spawner],
        "no entity was left behind"
    );
}

// ─── Placement ──────────────────────────────────────────────────────────────

#[test]
fn set_rotation_turns_the_entity() {
    let mut world = World::new();
    let entity = world.spawn(Transform::from_translation(Vec3::new(5.0, 0.0, 0.0)));

    let applied = run_and_apply(
        &mut world,
        "behavior Turret {
             void OnSpawn() { this.SetRotation(Quat(0.0, 0.6, 0.0, 0.8)); }
         }",
        "Turret.OnSpawn",
        entity,
    );

    assert_eq!(applied, 1);
    let transform = world.get::<Transform>(entity).expect("still placed");
    assert_eq!(
        transform.rotation,
        Quaternion {
            x: 0.0,
            y: 0.6,
            z: 0.0,
            w: 0.8,
        }
    );
    assert_eq!(
        transform.translation,
        Vec3::new(5.0, 0.0, 0.0),
        "turned, not moved"
    );
}

/// The method form reaches the world the way the free form did.
#[test]
fn a_method_call_moves_the_entity() {
    let mut world = World::new();
    let entity = world.spawn(Transform::identity());

    let applied = run_and_apply(
        &mut world,
        "behavior Mover {
             void OnSpawn() { this.SetPosition(Vec3(4.0, 5.0, 6.0)); }
         }",
        "Mover.OnSpawn",
        entity,
    );

    assert_eq!(applied, 1);
    assert_eq!(
        world.get::<Transform>(entity).expect("placed").translation,
        Vec3::new(4.0, 5.0, 6.0)
    );
}

// ─── What the mirrors refuse ────────────────────────────────────────────────

/// **`Transform` is placed, not written.** Its own commands allocate nothing
/// and look nothing up; a write of it is refused, and the refusal names them.
#[test]
fn transform_is_not_a_component_to_a_script() {
    let transform =
        "Transform { translation: Vec3(0.0, 1.0, 0.0), rotation: Quat(0.0, 0.0, 0.0, 1.0), scale: Vec3(1.0, 1.0, 1.0) }";
    for main in [
        format!("behavior Mover {{ void Go() {{ this.Set({transform}); }} }}"),
        format!("behavior Mover {{ void Go() {{ this.Add({transform}); }} }}"),
        format!("fn void Main() {{ Spawn(Vec3(0.0, 0.0, 0.0), {transform}); }}"),
    ] {
        let refusal = refused_saying(&main, "SetPosition");
        assert!(
            said(&refusal).contains("Transform"),
            "the refusal is about `Transform`; got {:?}",
            said(&refusal)
        );
    }
}

/// What the engine writes — here `BodyMotion`, the solver's velocity — is
/// served as a plain struct, and a struct is not a component a script writes.
#[test]
fn a_runtime_component_is_not_writable() {
    refused_saying(
        "behavior Pusher { void Go() { this.Set(BodyMotion { linear: Vec3(0.0, 9.0, 0.0), angular: Vec3(0.0, 0.0, 0.0) }); } }",
        "`BodyMotion` is not a component a script writes",
    );
}

/// **An add that is refused attaches nothing.** The patch over the defaults
/// refuses its value — a NaN, which no component stores — so the add is not
/// applied, and the entity must not be left holding a defaulted collider the
/// script never got: what `Spawn` rolls back, `Add` rolls back too.
#[test]
fn an_add_whose_patch_is_refused_attaches_nothing() {
    let mut world = World::new();
    let entity = world.spawn(Transform::identity());

    let applied = run_and_apply(
        &mut world,
        "behavior Anchor {
             void OnSpawn() {
                 float zero = 0.0;
                 this.Add(Collider { friction: zero / zero });
             }
         }",
        "Anchor.OnSpawn",
        entity,
    );

    assert_eq!(applied, 0, "the add was refused");
    assert!(
        world.get::<Collider>(entity).is_none(),
        "a refused add left a defaulted collider attached"
    );
}

/// Text the script built crosses into the component it writes.
#[test]
fn set_writes_text_the_script_built() {
    use khora_data::ui::UiText;

    let mut world = World::new();
    let entity = world.spawn((Transform::identity(), UiText::default()));

    let applied = run_and_apply(
        &mut world,
        r#"fn string Count(int n) { if (n > 2) { return "many"; } return "few"; }
           behavior Score {
               void OnSpawn() { this.Set(UiText { content: "hits: " + Count(3), size: 24.0 }); }
           }"#,
        "Score.OnSpawn",
        entity,
    );

    assert_eq!(applied, 1);
    let text = world.get::<UiText>(entity).expect("still a text");
    assert_eq!(text.content, "hits: many");
    assert_eq!(text.size, 24.0);
    assert_eq!(text.color, UiText::default().color, "a field left alone");
}

/// **What the mirror offers as a component, a script can attach.** Every
/// `component` the engine serves is added — its defaults, an empty patch —
/// to an entity that does not hold it, and spawned on a new one: each applies.
#[test]
fn every_mirrored_component_can_be_added_and_spawned() {
    let names: Vec<&str> = khora_io::script::mirror::mirror_source()
        .lines()
        .filter_map(|line| line.strip_prefix("component "))
        .filter_map(|rest| rest.split_whitespace().next())
        .collect();
    assert!(!names.is_empty(), "the mirror declares components");

    let mut refused = Vec::new();
    for name in names {
        let mut world = World::new();
        let entity = world.spawn(Transform::identity());
        let added = run_and_apply(
            &mut world,
            &format!("behavior Kit {{ void Go() {{ this.Add({name} {{ }}); }} }}"),
            "Kit.Go",
            entity,
        );
        if added != 1 {
            refused.push(format!("{name}: add applied {added}"));
        }

        let mut world = World::new();
        let spawner = world.spawn(Transform::identity());
        let spawned = run_and_apply(
            &mut world,
            &format!(
                "behavior Kit {{ void Go() {{ Spawn(Vec3(0.0, 0.0, 0.0), {name} {{ }}); }} }}"
            ),
            "Kit.Go",
            spawner,
        );
        if spawned != 1 {
            refused.push(format!("{name}: spawn applied {spawned}"));
        }
    }
    assert!(refused.is_empty(), "{}", refused.join("\n"));
}
