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

//! A script writes a camera's clip planes, end to end, and the values are
//! the script's.
//!
//! An authored value is never rewritten nor refused: a script moving a
//! camera's planes one write at a time passes through a camera whose near
//! sits beyond its far, and one writing a near of 0 leaves a camera that
//! cannot be projected. Neither stops the script nor changes what it wrote.
//! The components' own checks — the `component_checks` data system of the
//! `Maintenance` phase — say what is wrong with the world as it is after the
//! frame, and only that.
//!
//! Compiled against the mirrors the engine serves, run on the VM, applied to
//! a real `World` by the applier the `Maintenance` phase runs, then checked
//! by the registered data system.

use std::sync::Arc;

use khora_core::ecs::entity::EntityId;
use khora_core::lane::OutputDeck;
use khora_core::problems::{Problem, ProblemSeverity, Problems};
use khora_core::Runtime;
use khora_data::ecs::systems::script_commands::apply_all;
use khora_data::ecs::{Camera, DataSystemRegistration, World};
use khora_io::script::mirror::PreludeLoader;
use khora_script::vm::{Machine, Program, Run};
use khora_script::{compile_module, Diagnostic, MemoryLoader};

fn build(main: &str) -> Program {
    let source = format!("import \"engine/components.erg\";\n{main}");
    let loader = PreludeLoader::new(MemoryLoader::new().with("main.erg", source));
    let result = compile_module(&loader, "main.erg");
    let errors: Vec<&Diagnostic> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "should compile: {errors:?}");
    result.program.expect("a program with no errors")
}

/// Runs `member` as `entity`'s behavior to its end — no fault, no
/// suspension — then applies what it queued to `world`. Returns how many
/// commands were applied.
fn run_and_apply(world: &mut World, main: &str, member: &str, entity: EntityId) -> usize {
    let program = build(main);
    let mut host = khora_script::Host::new()
        .for_entity(entity)
        .with_reserver(world.reserver());
    let mut machine = Machine::new(&program, member, &[]).expect("the member exists");
    assert_eq!(
        machine.run(&program, &mut host, u64::MAX),
        Run::Completed,
        "the script runs to its end"
    );
    let queued = host.end_frame();
    world.settle_reservations();
    apply_all(world, &queued)
}

/// Runs the registered `component_checks` data system over `world` and
/// returns every problem the runtime's `Problems` then holds.
fn checked(world: &mut World) -> Vec<Problem> {
    let system = khora_script::inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == "component_checks")
        .expect("a `component_checks` data system is registered");
    let problems = Arc::new(Problems::default());
    let mut runtime = Runtime::default();
    runtime.resources.insert(Arc::clone(&problems));
    (system.run)(
        world,
        &runtime,
        &mut OutputDeck::new(),
        khora_data::ecs::SystemContext {
            since: khora_data::ecs::Tick(0),
        },
    );
    problems.all()
}

/// The problems held about `entity`'s `Camera`.
fn about_camera(problems: &[Problem], entity: EntityId) -> Vec<Problem> {
    problems
        .iter()
        .filter(|problem| {
            problem.entity == Some(entity) && problem.component.as_deref() == Some("Camera")
        })
        .cloned()
        .collect()
}

/// **Moving both planes, near first.** The far plane was 10; the script
/// writes `z_near = 50`, then `z_far = 100`. Between the two writes the
/// camera's near sits beyond its far — a state no one asked to be refused.
/// The script runs to its end, both values land as written, and the world
/// after the frame holds no problem.
#[test]
fn a_script_writing_near_then_far_ends_clean() {
    let mut world = World::new();
    let entity = world.spawn(Camera::new_perspective(1.0, 0.1, 10.0));

    let applied = run_and_apply(
        &mut world,
        "behavior Zoom {
             void OnSpawn() {
                 this.Set(Camera { z_near: 50.0 });
                 this.Set(Camera { z_far: 100.0 });
             }
         }",
        "Zoom.OnSpawn",
        entity,
    );

    assert_eq!(applied, 2, "both writes were applied");
    let camera = world.get::<Camera>(entity).expect("still a camera");
    assert_eq!(camera.z_near, 50.0);
    assert_eq!(camera.z_far, 100.0);
    let problems = checked(&mut world);
    assert!(problems.is_empty(), "{problems:?}");
}

/// **A near plane of 0 is the script's to write.** The script is not
/// stopped — the write after it lands too — the camera keeps the 0, and the
/// checks after the frame hold one error on that camera's `z_near`.
#[test]
fn a_script_writing_zero_near_is_not_stopped() {
    let mut world = World::new();
    let entity = world.spawn(Camera::new_perspective(1.0, 0.1, 10.0));

    let applied = run_and_apply(
        &mut world,
        "behavior Squash {
             void OnSpawn() {
                 this.Set(Camera { z_near: 0.0 });
                 this.Set(Camera { z_far: 200.0 });
             }
         }",
        "Squash.OnSpawn",
        entity,
    );

    assert_eq!(applied, 2, "the write after the 0 was applied too");
    let camera = world.get::<Camera>(entity).expect("still a camera");
    assert_eq!(camera.z_near.to_bits(), 0.0f32.to_bits(), "the 0 is kept");
    assert_eq!(camera.z_far, 200.0);

    let problems = checked(&mut world);
    let found = about_camera(&problems, entity);
    assert_eq!(found.len(), 1, "{problems:?}");
    assert_eq!(found[0].severity, ProblemSeverity::Error);
    assert_eq!(found[0].field.as_deref(), Some("z_near"));
    assert_eq!(problems.len(), 1, "nothing else: {problems:?}");
    assert_eq!(
        world
            .get::<Camera>(entity)
            .expect("still a camera")
            .z_near
            .to_bits(),
        0.0f32.to_bits(),
        "the checks rewrite nothing"
    );
}
