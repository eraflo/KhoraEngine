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

//! **A name changing kind on a hot reload** (Ergon spec 10) — a behavior
//! become a system, a system become a behavior: the system runs once a
//! frame, on its one entity, whatever entities held the behavior; the
//! entities a scene authored stay in the scene; and a system become a
//! behavior gives up its entity while the behavior's holders run.

use khora_core::agent::EngineMode;
use khora_core::scene::SerializationGoal;
use khora_core::script::ScriptValue;
use khora_data::ecs::{Transform, World};
use khora_io::serialization::SerializationService;

use super::behavior_session::{attach, int, Project};
use super::system_session::{singleton, SystemSession};

const COUNTER: (&str, &str) = ("game/counter.erg", "component Counter { int n = 0; }");

/// `Pulse` a behavior: each holder adds one to every `Counter` a frame.
const AS_BEHAVIOR: Project = &[
    COUNTER,
    (
        "game/pulse.erg",
        "import \"game/counter.erg\";
         behavior Pulse {
             void Update(float dt) { foreach (Entity e, Counter c) { e.Set(Counter { n: c.n + 1 }); } }
         }",
    ),
];

/// `Pulse` a system: once a frame, one added to every `Counter`.
const AS_SYSTEM: Project = &[
    COUNTER,
    (
        "game/pulse.erg",
        "import \"game/counter.erg\";
         system Pulse {
             void Update(float dt) { foreach (ref Counter c) { c.n += 1; } }
         }",
    ),
];

/// **A behavior become a system runs once a frame**: two entities held the
/// behavior `Pulse`; once it is a system, a frame adds one to the counter —
/// the system's one instance — not one per entity that held the behavior.
#[test]
fn a_behavior_become_a_system_runs_once_a_frame_whatever_held_it() {
    let mut session = SystemSession::new(World::new(), AS_BEHAVIOR);
    let counter = session.world.spawn(Transform::identity());
    attach(&mut session.world, counter, "Counter", &[]);
    for _ in 0..2 {
        let holder = session.world.spawn(Transform::identity());
        attach(&mut session.world, holder, "Pulse", &[]);
    }
    session.frame(0.016);
    assert_eq!(
        int(&session.world, counter, "Counter", "n"),
        Some(2),
        "as a behavior, each of its two holders adds one"
    );

    session.recompile(AS_SYSTEM, &["game/pulse.erg"]);
    session.frame(0.016);
    session.frame(0.016);

    assert!(singleton(&session.world, "Pulse").is_some(), "its entity");
    assert_eq!(
        int(&session.world, counter, "Counter", "n"),
        Some(4),
        "as a system, one a frame"
    );
}

/// **An authored entity that held a behavior become a system stays in the
/// scene**: edited in the editor — nothing playing — the behavior `Pulse`
/// turns into a system; the scene saved then still holds the authored
/// entity that held it, and its `Counter`.
#[test]
fn an_authored_holder_of_a_behavior_become_a_system_stays_in_the_scene() {
    let service = SerializationService::new();
    let mut session = SystemSession::in_mode(
        World::new(),
        AS_BEHAVIOR,
        EngineMode::Custom("editor".to_owned()),
    );
    let holder = session.world.spawn(Transform::identity());
    attach(
        &mut session.world,
        holder,
        "Counter",
        &[("n", ScriptValue::Int(7))],
    );
    attach(&mut session.world, holder, "Pulse", &[]);
    let id = session.world.mark_authored(holder).expect("authored");
    session.frame(0.016);

    session.recompile(AS_SYSTEM, &["game/pulse.erg"]);
    session.frame(0.016);

    let scene = service
        .save_world(&session.world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    let mut loaded = World::new();
    service
        .load_world(&scene, &mut loaded)
        .expect("the scene loads");
    let back = loaded
        .entity_with_id(id)
        .expect("the authored entity is still in the scene");
    assert_eq!(int(&loaded, back, "Counter", "n"), Some(7));
}

/// **A system become a behavior gives up its entity, and the behavior's
/// holders run**: while playing, `Pulse` turns from a system into a
/// behavior held by one entity; its singleton is despawned and the holder
/// adds one a frame.
#[test]
fn a_system_become_a_behavior_drops_its_entity_and_its_holders_run() {
    let mut session = SystemSession::new(World::new(), AS_SYSTEM);
    let counter = session.world.spawn(Transform::identity());
    attach(&mut session.world, counter, "Counter", &[]);
    session.frame(0.016);
    let single = singleton(&session.world, "Pulse").expect("its entity");
    assert_eq!(int(&session.world, counter, "Counter", "n"), Some(1));

    session.recompile(AS_BEHAVIOR, &["game/pulse.erg"]);
    session.frame(0.016);
    let holder = session.world.spawn(Transform::identity());
    attach(&mut session.world, holder, "Pulse", &[]);
    let before = int(&session.world, counter, "Counter", "n").expect("n");
    session.frame(0.016);
    session.frame(0.016);

    assert!(
        !session.world.contains(single),
        "the singleton is despawned"
    );
    assert_eq!(singleton(&session.world, "Pulse"), None);
    assert_eq!(
        int(&session.world, counter, "Counter", "n"),
        Some(before + 2),
        "the one holder, one a frame"
    );
}

/// `Pulse` a plain component: no program runs it.
const AS_COMPONENT: Project = &[
    COUNTER,
    ("game/pulse.erg", "component Pulse { int beats = 0; }"),
];

/// **A system become a plain component gives up its entity**: while
/// playing, `Pulse` turns from a system into a component; the system's
/// entity is despawned — no system is left for it to be — and a scene saved
/// then holds no entity of that name.
#[test]
fn a_system_become_a_component_drops_its_entity() {
    let service = SerializationService::new();
    let mut session = SystemSession::new(World::new(), AS_SYSTEM);
    let counter = session.world.spawn(Transform::identity());
    attach(&mut session.world, counter, "Counter", &[]);
    session.frame(0.016);
    let single = singleton(&session.world, "Pulse").expect("its entity");

    session.recompile(AS_COMPONENT, &["game/pulse.erg"]);
    session.frame(0.016);
    session.frame(0.016);

    assert!(
        !session.world.contains(single),
        "the entity of a system that is no more is despawned"
    );
    let scene = service
        .save_world(&session.world, SerializationGoal::EditorInterchange)
        .expect("the scene saves");
    let mut loaded = World::new();
    service
        .load_world(&scene, &mut loaded)
        .expect("the scene loads");
    assert_eq!(singleton(&loaded, "Pulse"), None, "no scene holds it");
}
