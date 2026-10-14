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

//! **A system runs as a behavior instance on its one entity** (Ergon spec
//! 10): its `Update` every frame while the game plays, its fields kept from
//! frame to frame, its `every` and `after` firing on game time, a fault
//! disabling it, a hot reload carrying its fields — and behaviors and
//! systems taking their turns in one list, by module and then by where each
//! is written.

use khora_core::agent::EngineMode;
use khora_core::script::ScriptValue;
use khora_data::ecs::{Transform, World};

use super::behavior_session::{attach, field, int, Project};
use super::system_session::{holders, the_holder, SystemSession};

const CLOCK: Project = &[(
    "game/clock.erg",
    "system Clock {
         int ticks = 0;
         float elapsed = 0.0;
         void Update(float dt) { ticks += 1; elapsed += dt; }
     }",
)];

/// **A system's `Update` runs every frame while playing, and its fields last
/// from one frame to the next.**
#[test]
fn a_systems_update_runs_every_frame_and_its_fields_persist() {
    let mut session = SystemSession::new(World::new(), CLOCK);
    for _ in 0..3 {
        session.frame(0.25);
    }

    let clock = the_holder(&session.world, "Clock");
    assert_eq!(int(&session.world, clock, "Clock", "ticks"), Some(3));
    assert_eq!(
        field(&session.world, clock, "Clock", "elapsed"),
        Some(ScriptValue::Float(0.75)),
        "the frames' time, summed"
    );
}

/// **A system does not run while the game does not play**: an editor
/// editing a scene runs no system, and its first frame of Play does.
#[test]
fn a_system_runs_only_while_playing() {
    let mut session =
        SystemSession::in_mode(World::new(), CLOCK, EngineMode::Custom("editor".to_owned()));
    for _ in 0..3 {
        session.frame(0.25);
    }
    assert_eq!(
        holders(&session.world, "Clock").map(|held| held.len()),
        Some(0),
        "registered, and held by nothing while editing"
    );

    session.set_mode(EngineMode::Playing);
    session.frame(0.25);
    session.frame(0.25);
    let clock = the_holder(&session.world, "Clock");
    assert_eq!(int(&session.world, clock, "Clock", "ticks"), Some(2));
}

const PULSE: Project = &[(
    "game/pulse.erg",
    "system Pulse {
         int pulses = 0;
         int late = 0;
         every 0.25s { pulses += 1; }
         after 1s { late += 1; }
     }",
)];

/// **`every` and `after` fire on game time**: over 1.2 s in tenths, the
/// quarter-second timer four times and the one-second one once.
#[test]
fn a_systems_timers_fire() {
    let mut session = SystemSession::new(World::new(), PULSE);
    for _ in 0..12 {
        session.frame(0.1);
    }

    let pulse = the_holder(&session.world, "Pulse");
    assert_eq!(int(&session.world, pulse, "Pulse", "pulses"), Some(4));
    assert_eq!(int(&session.world, pulse, "Pulse", "late"), Some(1));
}

const FRAGILE: Project = &[(
    "game/fragile.erg",
    "system Fragile {
         int ticks = 0;
         int[] none;
         void Update(float dt) { ticks += 1; if (ticks == 2) { int x = none[5]; } }
     }",
)];

/// **A fault disables a system like a behavior**: from the frame its
/// `Update` faults, it runs no more, and its state records the fault.
#[test]
fn a_faulting_system_is_disabled() {
    let mut session = SystemSession::new(World::new(), FRAGILE);
    for _ in 0..5 {
        session.frame(0.1);
    }

    let fragile = the_holder(&session.world, "Fragile");
    assert_eq!(
        int(&session.world, fragile, "Fragile", "ticks"),
        Some(2),
        "it ran until it faulted, then stopped"
    );
    assert_ne!(
        field(&session.world, fragile, "Fragile#state", "fault"),
        Some(ScriptValue::Null),
        "the fault is recorded"
    );
}

const CLOCK_EDITED: Project = &[(
    "game/clock.erg",
    "system Clock {
         int ticks = 0;
         int bonus = 7;
         float elapsed = 0.0;
         void Update(float dt) { ticks += 10; }
     }",
)];

/// **A hot reload carries a system's fields**: the edited `Update` adds to
/// what the system held, a new field starts at its default, and the system
/// is still one entity.
#[test]
fn a_hot_reload_carries_a_systems_fields() {
    let mut session = SystemSession::new(World::new(), CLOCK);
    for _ in 0..3 {
        session.frame(0.1);
    }
    let before = the_holder(&session.world, "Clock");

    session.recompile(CLOCK_EDITED, &["game/clock.erg"]);
    session.frame(0.1);

    let clock = the_holder(&session.world, "Clock");
    assert_eq!(clock, before, "the same entity");
    assert_eq!(
        int(&session.world, clock, "Clock", "ticks"),
        Some(13),
        "three ticks carried, then the edited Update"
    );
    assert_eq!(int(&session.world, clock, "Clock", "bonus"), Some(7));
}

// ─── One list ───────────────────────────────────────────────────────────────

/// Each script appends its digit to the entity's `Mark.log`, reading what
/// the scripts before it this frame wrote: `game/a.erg` writes `Zulu` (1),
/// `Mike` (2) and `Alpha` (3) in that order — names sorting the other way —
/// and `game/b.erg` writes `Bravo` (4).
const ORDERED: Project = &[
    ("game/mark.erg", "component Mark { int log = 0; }"),
    (
        "game/a.erg",
        "import \"game/mark.erg\";
         behavior Zulu {
             void Update(float dt) {
                 if (var m = this.Get(Mark)) { this.Set(Mark { log: (m.log * 10 + 1) % 10000 }); }
             }
         }
         system Mike {
             void Update(float dt) {
                 foreach (Entity e, Mark m) { e.Set(Mark { log: (m.log * 10 + 2) % 10000 }); }
             }
         }
         behavior Alpha {
             void Update(float dt) {
                 if (var m = this.Get(Mark)) { this.Set(Mark { log: (m.log * 10 + 3) % 10000 }); }
             }
         }",
    ),
    (
        "game/b.erg",
        "import \"game/mark.erg\";
         system Bravo {
             void Update(float dt) {
                 foreach (Entity e, Mark m) { e.Set(Mark { log: (m.log * 10 + 4) % 10000 }); }
             }
         }",
    ),
];

/// **Behaviors and systems take their turns in one list, by module then by
/// source order**: each frame's log spells `Zulu`, `Mike`, `Alpha`, `Bravo`.
#[test]
fn behaviors_and_systems_run_by_module_then_source_order() {
    let mut session = SystemSession::new(World::new(), ORDERED);
    let marked = session.world.spawn(Transform::identity());
    attach(&mut session.world, marked, "Mark", &[]);
    attach(&mut session.world, marked, "Alpha", &[]);
    attach(&mut session.world, marked, "Zulu", &[]);

    session.frame(0.016);
    session.frame(0.016);

    assert_eq!(
        int(&session.world, marked, "Mark", "log"),
        Some(1234),
        "Zulu, Mike, Alpha (game/a.erg as written), then Bravo (game/b.erg)"
    );
}
