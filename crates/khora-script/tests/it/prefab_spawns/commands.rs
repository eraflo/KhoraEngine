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

//! What a prefab spawn queues, and when it faults instead.

use std::collections::BTreeSet;

use khora_core::math::{Quaternion, Vec3};

use super::*;

/// Spawns `bullet` with a `Health`, at a position, and returns the root.
const FIRE: &str = "fn Entity Fire(Prefab bullet) {
    return Spawn(bullet, Health { current: 7, max: 10 }, at: Vec3(1.0, 2.0, 3.0));
}";

/// **A prefab spawn queues one `SpawnPrefab`**, carrying the id it returned,
/// the prefab, the position, no rotation — the root keeps the prefab's — and
/// the components written in the call.
#[test]
fn a_prefab_spawn_queues_its_command_with_the_root_it_returns() {
    let program = build(FIRE);

    let (result, commands) = run(&program, "Fire", &[Value::Asset(bullet())]);

    let root = entity_of(&result);
    assert_eq!(
        commands,
        vec![WorldCommand::SpawnPrefab {
            entity: root,
            prefab: bullet(),
            position: Vec3::new(1.0, 2.0, 3.0),
            rotation: None,
            components: vec![(
                ComponentName::new("Health"),
                fields(&[
                    ("current", ScriptValue::Int(7)),
                    ("max", ScriptValue::Int(10)),
                ]),
            )],
        }]
    );
    assert_ne!(root, SUBJECT, "the root is a new entity");
}

/// **`facing:` is the root's rotation**, carried as given.
#[test]
fn facing_is_carried_as_the_roots_rotation() {
    let program = build(
        "fn Entity Fire(Prefab bullet) {
             return Spawn(bullet, at: Vec3(0.0, 0.0, 0.0), facing: Quat(0.0, 0.6, 0.0, 0.8));
         }",
    );

    let (_, commands) = run(&program, "Fire", &[Value::Asset(bullet())]);

    match commands.as_slice() {
        [WorldCommand::SpawnPrefab {
            rotation,
            components,
            ..
        }] => {
            assert_eq!(*rotation, Some(Quaternion::new(0.0, 0.6, 0.0, 0.8)));
            assert!(components.is_empty(), "{components:?}");
        }
        other => panic!("one prefab spawn, not {other:?}"),
    }
}

/// **A `Prefab?` that holds a prefab spawns it**, as a `Prefab` would.
#[test]
fn an_optional_prefab_holding_one_spawns_it() {
    let program = build(
        "fn Entity Fire(Prefab? bullet) {
             return Spawn(bullet, at: Vec3(0.0, 1.0, 0.0));
         }",
    );

    let (result, commands) = run(&program, "Fire", &[Value::Asset(bullet())]);

    assert_eq!(
        commands,
        vec![WorldCommand::SpawnPrefab {
            entity: entity_of(&result),
            prefab: bullet(),
            position: Vec3::new(0.0, 1.0, 0.0),
            rotation: None,
            components: Vec::new(),
        }]
    );
}

/// **A `null` prefab faults the turn**, naming the argument as written, and
/// queues nothing.
#[test]
fn a_null_prefab_faults_naming_the_argument() {
    let program = build(
        "fn Entity Fire(Prefab? bullet) {
             return Spawn(bullet, Health { current: 1 }, at: Vec3(0.0, 0.0, 0.0));
         }",
    );

    let ran = run_on(&program, "Fire", &[Value::Null], host());

    let fault = fault_of(&ran);
    assert!(fault.contains("`bullet` is `null`"), "{fault}");
    assert!(ran.commands.is_empty(), "{:?}", ran.commands);
}

/// A `null` prefab that is not a name is "this prefab".
#[test]
fn a_null_prefab_that_is_not_a_name_is_this_prefab() {
    let program = build(
        "fn Prefab? Pick() { return null; }
         fn Entity Fire() {
             return Spawn(Pick(), at: Vec3(0.0, 0.0, 0.0));
         }",
    );

    let ran = run_on(&program, "Fire", &[], host());

    let fault = fault_of(&ran);
    assert!(fault.contains("this prefab is `null`"), "{fault}");
    assert!(ran.commands.is_empty(), "{:?}", ran.commands);
}

/// **A prefab the catalog does not hold faults the turn** — "`bullet` names
/// no prefab" — and queues nothing.
#[test]
fn a_prefab_the_catalog_does_not_hold_faults() {
    let program = build(FIRE);
    let catalog = Arc::new(BTreeSet::from([other()]));

    let ran = run_on(
        &program,
        "Fire",
        &[Value::Asset(bullet())],
        host().with_prefabs(catalog),
    );

    let fault = fault_of(&ran);
    assert!(fault.contains("`bullet` names no prefab"), "{fault}");
    assert!(ran.commands.is_empty(), "{:?}", ran.commands);
}

/// A prefab the catalog holds spawns.
#[test]
fn a_prefab_the_catalog_holds_spawns() {
    let program = build(FIRE);
    let catalog = Arc::new(BTreeSet::from([other(), bullet()]));

    let ran = run_on(
        &program,
        "Fire",
        &[Value::Asset(bullet())],
        host().with_prefabs(catalog),
    );

    assert_eq!(ran.run, Run::Completed);
    assert_eq!(prefab_roots(&ran.commands), vec![entity_of(&ran.result)]);
}

/// **A host given no catalog cannot check**, and accepts any prefab: the
/// boundary refuses one no project holds.
#[test]
fn without_a_catalog_any_prefab_is_accepted() {
    let program = build(FIRE);
    let unknown = AssetUUID::new();

    let (result, commands) = run(&program, "Fire", &[Value::Asset(unknown)]);

    match commands.as_slice() {
        [WorldCommand::SpawnPrefab { entity, prefab, .. }] => {
            assert_eq!(*prefab, unknown);
            assert_eq!(*entity, entity_of(&result));
        }
        other => panic!("one prefab spawn, not {other:?}"),
    }
}

/// **Two prefab spawns are two roots**, each its own id.
#[test]
fn two_prefab_spawns_are_two_roots() {
    let program = build(
        "fn Entity Fire(Prefab bullet) {
             var a = Spawn(bullet, at: Vec3(0.0, 0.0, 0.0));
             var b = Spawn(bullet, at: Vec3(1.0, 0.0, 0.0));
             return b;
         }",
    );

    let (result, commands) = run(&program, "Fire", &[Value::Asset(bullet())]);

    let roots = prefab_roots(&commands);
    assert_eq!(roots.len(), 2, "{commands:?}");
    assert_ne!(roots[0], roots[1]);
    assert_eq!(entity_of(&result), roots[1]);
}

/// **What follows in the turn names the root**: a `Set` after the spawn is
/// queued after it, for the root's id.
#[test]
fn a_set_after_a_prefab_spawn_names_the_root() {
    let program = build(
        "fn void Fire(Prefab bullet) {
             var b = Spawn(bullet, Health { current: 7, max: 10 }, at: Vec3(0.0, 0.0, 0.0));
             b.Set(Health { current: 3 });
         }",
    );

    let (_, commands) = run(&program, "Fire", &[Value::Asset(bullet())]);

    let roots = prefab_roots(&commands);
    assert_eq!(roots.len(), 1, "{commands:?}");
    assert_eq!(commands.len(), 2, "{commands:?}");
    assert_eq!(
        commands[1],
        WorldCommand::SetComponent {
            entity: roots[0],
            component: ComponentName::new("Health"),
            value: fields(&[("current", ScriptValue::Int(3))]),
        }
    );
}

/// **What the call wrote is readable in the same turn**: `b.Get` reads the
/// component the spawn wrote — its unwritten fields from the default row —
/// and `b.Has` holds it. A component the call did not write is not read: the
/// prefab's own are the next frame's.
#[test]
fn the_written_components_are_readable_in_the_same_turn() {
    let program = build(
        "fn int Fire(Prefab bullet) {
             var b = Spawn(bullet, Health { current: 7 }, at: Vec3(0.0, 0.0, 0.0));
             int answer = 0;
             if (var h = b.Get(Health)) { answer = h.current * 1000 + h.max; }
             if (b.Has(Health)) { answer += 1; }
             if (b.Has(Collider)) { answer += 10; }
             return answer;
         }",
    );

    let (result, _) = run(&program, "Fire", &[Value::Asset(bullet())]);

    assert_eq!(result, Value::Int(7101));
}

/// A `Set` after the spawn merges onto what the call wrote, in the turn.
#[test]
fn a_set_after_a_prefab_spawn_merges_onto_what_it_wrote() {
    let program = build(
        "fn int Fire(Prefab bullet) {
             var b = Spawn(bullet, Health { current: 7, max: 10 }, at: Vec3(0.0, 0.0, 0.0));
             b.Set(Health { current: 3 });
             if (var h = b.Get(Health)) { return h.current * 1000 + h.max; }
             return -1;
         }",
    );

    let (result, _) = run(&program, "Fire", &[Value::Asset(bullet())]);

    assert_eq!(result, Value::Int(3010));
}

/// **A prefab spawn reserves its root once, however its run is cut.** At
/// every slice of fuel the run queues one prefab spawn and a `Set` of the
/// root it carries, and returns that root.
#[test]
fn a_prefab_spawn_reserves_once_however_its_run_is_sliced() {
    let program = build(
        "fn Entity Fire(Prefab bullet) {
             var b = Spawn(bullet, Health { current: 7, max: 10 }, at: Vec3(0.0, 0.0, 0.0));
             b.Set(Health { current: 3 });
             return b;
         }",
    );

    for slice in 1..=40 {
        let mut host = host();
        let mut machine =
            Machine::new(&program, "Fire", &[Value::Asset(bullet())]).expect("`Fire` exists");
        let mut completed = false;
        for _ in 0..100_000 {
            match machine.run(&program, &mut host, slice) {
                Run::Completed => {
                    completed = true;
                    break;
                }
                Run::Suspended(_) => {}
                Run::Faulted(fault) => panic!("faulted at a slice of {slice}: {fault:?}"),
            }
        }
        assert!(completed, "never completed at a slice of {slice}");
        let commands = host.commands.as_slice().to_vec();
        let roots = prefab_roots(&commands);
        assert_eq!(roots.len(), 1, "at a slice of {slice}: {commands:?}");
        assert_eq!(entity_of(&machine.result()), roots[0]);
        assert_eq!(
            commands.last().and_then(WorldCommand::entity),
            Some(roots[0]),
            "at a slice of {slice}, the `Set` names the root"
        );
    }
}

/// Calls that each queue a write of `e` before giving their value: the
/// order their writes are queued in is the order they ran in.
const ORDERED: &str = "fn Vec3 Where(Entity e) {
         e.Set(Collider { friction: 1.0 });
         return Vec3(0.0, 0.0, 0.0);
     }
     fn Quat Turn(Entity e) {
         e.Set(Collider { is_sensor: true });
         return Quat(0.0, 0.0, 0.0, 1.0);
     }
     fn int Mark(Entity e) {
         e.Set(Health { max: 1 });
         return 5;
     }";

/// The components the writes queued among `commands` name, in order.
fn written(commands: &[WorldCommand]) -> Vec<ScriptValue> {
    commands
        .iter()
        .filter_map(|command| match command {
            WorldCommand::SetComponent { value, .. } => Some(value.clone()),
            _ => None,
        })
        .collect()
}

/// **A prefab spawn's arguments run in the order they are written**, as
/// every call with named arguments does: a component written before `at:`
/// runs before it.
#[test]
fn a_prefab_spawns_components_run_before_its_at() {
    let program = build(&format!(
        "{ORDERED}
         fn Entity Fire(Prefab bullet, Entity e) {{
             return Spawn(bullet, Health {{ current: Mark(e) }}, at: Where(e));
         }}"
    ));

    let (_, commands) = run(
        &program,
        "Fire",
        &[Value::Asset(bullet()), Value::Entity(SUBJECT)],
    );

    assert_eq!(
        written(&commands),
        vec![
            fields(&[("max", ScriptValue::Int(1))]),
            fields(&[("friction", ScriptValue::Float(1.0))]),
        ],
        "`Mark` is written first and ran second: {commands:?}"
    );
}

/// **`facing:` written before `at:` runs before it.**
#[test]
fn a_prefab_spawns_facing_written_first_runs_first() {
    let program = build(&format!(
        "{ORDERED}
         fn Entity Fire(Prefab bullet, Entity e) {{
             return Spawn(bullet, facing: Turn(e), at: Where(e));
         }}"
    ));

    let (_, commands) = run(
        &program,
        "Fire",
        &[Value::Asset(bullet()), Value::Entity(SUBJECT)],
    );

    assert_eq!(
        written(&commands),
        vec![
            fields(&[("is_sensor", ScriptValue::Bool(true))]),
            fields(&[("friction", ScriptValue::Float(1.0))]),
        ],
        "`Turn` is written first and ran second: {commands:?}"
    );
}

/// A prefab spawn whose place comes from a call — a turn cut inside it
/// stands in the middle of the argument list — and the same function with a
/// statement written above the spawn.
const PLACED_BY_A_CALL: (&str, &str) = (
    "fn Vec3 Where() { int a = 1; int b = a + 1; return Vec3(1.0, 2.0, 3.0); }
     fn Entity Fire(Prefab bullet) {
         return Spawn(bullet, Health { current: 7 }, at: Where());
     }",
    "fn Vec3 Where() { int a = 1; int b = a + 1; return Vec3(1.0, 2.0, 3.0); }
     fn Entity Fire(Prefab bullet) {
         int pad = 0;
         return Spawn(bullet, Health { current: 7 }, at: Where());
     }",
);

/// **A turn cut in the middle of a prefab spawn's arguments, saved and
/// resumed — into the same code or code edited above the spawn — spawns
/// the prefab once**, at the place the call gave, at every slice.
#[test]
fn a_prefab_spawn_cut_inside_its_arguments_resumes_into_one_spawn() {
    use khora_core::script::PendingBody;
    use khora_script::vm::resume;

    let (original, edited) = PLACED_BY_A_CALL;
    let original = build(original);
    let edited = build(edited);

    for target in [&original, &edited] {
        for slice in 1..=30 {
            let mut host = host();
            let mut machine =
                Machine::new(&original, "Fire", &[Value::Asset(bullet())]).expect("`Fire` exists");
            let mut program = &original;
            let mut moved = false;
            let mut completed = false;
            for _ in 0..10_000 {
                match machine.run(program, &mut host, slice) {
                    Run::Completed => {
                        completed = true;
                        break;
                    }
                    Run::Suspended(_) if !moved => {
                        let frozen = machine
                            .freeze(program, PendingBody::Sequence)
                            .expect("a cut freezes");
                        let (resumed, _) = resume(&frozen, program.fingerprint(), target)
                            .unwrap_or_else(|_| panic!("abandoned at a slice of {slice}"));
                        machine = resumed;
                        program = target;
                        moved = true;
                    }
                    Run::Suspended(_) => {}
                    Run::Faulted(fault) => panic!("faulted at a slice of {slice}: {fault:?}"),
                }
            }
            assert!(completed, "never completed at a slice of {slice}");
            let commands = host.commands.as_slice().to_vec();
            match commands.as_slice() {
                [WorldCommand::SpawnPrefab {
                    entity,
                    prefab,
                    position,
                    ..
                }] => {
                    assert_eq!(*prefab, bullet(), "at a slice of {slice}");
                    assert_eq!(*position, Vec3::new(1.0, 2.0, 3.0), "at a slice of {slice}");
                    assert_eq!(entity_of(&machine.result()), *entity);
                }
                other => panic!("one prefab spawn at a slice of {slice}, not {other:?}"),
            }
        }
    }
}

/// **An `int` written for a `float` field arrives as that float**, in a
/// prefab spawn's component as in a plain spawn's.
#[test]
fn an_int_written_for_a_float_field_of_a_prefab_spawn_is_widened() {
    let program = build(
        "fn Entity Fire(Prefab bullet) {
             return Spawn(bullet, Collider { friction: 1, is_sensor: true }, at: Vec3(0.0, 0.0, 0.0));
         }",
    );

    let (_, commands) = run(&program, "Fire", &[Value::Asset(bullet())]);

    match commands.as_slice() {
        [WorldCommand::SpawnPrefab { components, .. }] => assert_eq!(
            components,
            &vec![(
                ComponentName::new("Collider"),
                fields(&[
                    ("friction", ScriptValue::Float(1.0)),
                    ("is_sensor", ScriptValue::Bool(true)),
                ]),
            )]
        ),
        other => panic!("one prefab spawn, not {other:?}"),
    }
}

/// **A field that faults takes no id**: the next id the world hands out is
/// the one a run that never spawned would have been handed.
#[test]
fn a_prefab_spawn_whose_field_faults_takes_no_id() {
    let program = build(
        "fn Entity Fire(Prefab bullet, int zero) {
             return Spawn(bullet, Health { current: 1 / zero }, at: Vec3(0.0, 0.0, 0.0));
         }",
    );
    let host = host();
    let reserver = Arc::clone(&host.reserver);

    let ran = run_on(
        &program,
        "Fire",
        &[Value::Asset(bullet()), Value::Int(0)],
        host,
    );

    assert!(matches!(ran.run, Run::Faulted(_)), "{:?}", ran.run);
    assert!(ran.commands.is_empty(), "{:?}", ran.commands);
    let fresh = khora_core::ecs::EntityReserver::new();
    assert_eq!(reserver.reserve(), fresh.reserve(), "no id was taken");
}

/// Values two calls each give a field, the first cut by a small fuel slice.
const FIELDS_FROM_CALLS: &str = "fn int Seven() { int a = 3; int b = a + 4; return b; }
     fn int Ten() { int a = 4; int b = a + 6; return b; }
     fn Entity Fire(Prefab bullet) {
         return Spawn(bullet, Health { current: Seven(), max: Ten() }, facing: Quat(0.0, 0.6, 0.0, 0.8), at: Vec3(1.0, 2.0, 3.0));
     }";

/// **A turn cut inside a prefab spawn's component fields, frozen and resumed
/// at every slice, spawns once with every argument where it belongs.**
#[test]
fn a_prefab_spawn_cut_inside_its_fields_resumes_with_every_argument_in_place() {
    use khora_core::script::PendingBody;
    use khora_script::vm::resume;

    let program = build(FIELDS_FROM_CALLS);

    for slice in 1..=40 {
        let mut host = host();
        let mut machine =
            Machine::new(&program, "Fire", &[Value::Asset(bullet())]).expect("`Fire` exists");
        let mut completed = false;
        for _ in 0..10_000 {
            match machine.run(&program, &mut host, slice) {
                Run::Completed => {
                    completed = true;
                    break;
                }
                Run::Suspended(_) => {
                    let frozen = machine
                        .freeze(&program, PendingBody::Sequence)
                        .expect("a cut freezes");
                    let (resumed, _) = resume(&frozen, program.fingerprint(), &program)
                        .unwrap_or_else(|_| panic!("abandoned at a slice of {slice}"));
                    machine = resumed;
                }
                Run::Faulted(fault) => panic!("faulted at a slice of {slice}: {fault:?}"),
            }
        }
        assert!(completed, "never completed at a slice of {slice}");
        let commands = host.commands.as_slice().to_vec();
        assert_eq!(
            commands,
            vec![WorldCommand::SpawnPrefab {
                entity: entity_of(&machine.result()),
                prefab: bullet(),
                position: Vec3::new(1.0, 2.0, 3.0),
                rotation: Some(Quaternion::new(0.0, 0.6, 0.0, 0.8)),
                components: vec![(
                    ComponentName::new("Health"),
                    fields(&[
                        ("current", ScriptValue::Int(7)),
                        ("max", ScriptValue::Int(10)),
                    ]),
                )],
            }],
            "at a slice of {slice}"
        );
    }
}
