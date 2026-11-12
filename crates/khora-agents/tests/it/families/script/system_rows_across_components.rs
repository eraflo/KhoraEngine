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

//! **Sweeps of two components, interleaved** (Ergon spec 10): a run writing
//! `Health`, then `Regen`, then `Health` again leaves the `World` holding
//! each field as the run last wrote it — the later `Health` sweep reading
//! what the `Regen` sweep wrote — whether the run finishes in one frame or
//! is cut by fuel anywhere.

use khora_core::ecs::entity::EntityId;
use khora_core::script::ScriptValue;
use khora_data::ecs::{Transform, World};

use super::behavior_session::{attach, int, Project};
use super::system_session::SystemSession;

const INTERLEAVED: Project = &[
    (
        "game/stats.erg",
        "component Health { int current = 0; int max = 10; }
         component Regen { int rate = 0; }",
    ),
    (
        "game/heal.erg",
        "import \"game/stats.erg\";
         system Heal {
             int runs = 0;
             void Update(float dt) {
                 if (runs > 0) { return; }
                 foreach (ref Health h) { h.current += 1; }
                 foreach (ref Regen r) { r.rate += 2; }
                 foreach (ref Health h, Regen r) { h.current = h.current * r.rate; }
                 runs += 1;
             }
         }",
    ),
];

/// Three entities holding `Health { current: n }` and `Regen { rate: n }`.
fn stats(session: &mut SystemSession) -> Vec<EntityId> {
    (1..=3)
        .map(|n| {
            let entity = session.world.spawn(Transform::identity());
            attach(
                &mut session.world,
                entity,
                "Health",
                &[("current", ScriptValue::Int(n))],
            );
            attach(
                &mut session.world,
                entity,
                "Regen",
                &[("rate", ScriptValue::Int(n))],
            );
            entity
        })
        .collect()
}

/// One `Update` from `(current, rate)`.
fn update(current: i64, rate: i64) -> (i64, i64) {
    let rate = rate + 2;
    ((current + 1) * rate, rate)
}

/// What the World holds of each entity's `(current, rate)`.
fn held(session: &SystemSession, entities: &[EntityId]) -> Vec<(i64, i64)> {
    entities
        .iter()
        .map(|&entity| {
            (
                int(&session.world, entity, "Health", "current").expect("current"),
                int(&session.world, entity, "Regen", "rate").expect("rate"),
            )
        })
        .collect()
}

/// **Each field is left as the run last wrote it**: with fuel to spare, a
/// frame is one `Update`, and the `World` after it holds `(n + 1) * (n + 2)`.
#[test]
fn interleaved_sweeps_of_two_components_leave_each_last_write() {
    let mut session = SystemSession::new(World::new(), INTERLEAVED);
    let entities = stats(&mut session);
    session.frame(0.016);
    let expected: Vec<(i64, i64)> = (1..=3).map(|n| update(n, n)).collect();
    assert_eq!(held(&session, &entities), expected);
}

/// **Cut by fuel anywhere, the same writes reach the `World`**: for every
/// fuel slice from 1 up, the frames that finish the first `Update` leave the
/// `World` where one `Update` with fuel to spare does.
#[test]
fn interleaved_sweeps_cut_by_any_fuel_slice_leave_each_last_write() {
    let expected: Vec<(i64, i64)> = (1..=3).map(|n| update(n, n)).collect();
    for fuel in 1..=120 {
        let mut session = SystemSession::new(World::new(), INTERLEAVED);
        let entities = stats(&mut session);
        session.fuel = fuel;
        let mut frames = 0;
        loop {
            session.frame(0.016);
            frames += 1;
            assert!(
                frames < 10_000,
                "fuel {fuel}: the first `Update` never finishes"
            );
            let heal = super::system_session::the_holder(&session.world, "Heal");
            if int(&session.world, heal, "Heal", "runs") == Some(1) {
                break;
            }
        }
        assert_eq!(held(&session, &entities), expected, "fuel {fuel}");
    }
}
