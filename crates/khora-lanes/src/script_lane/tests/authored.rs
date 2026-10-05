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

//! An author's edit to `Script.fields` against what the game observed.
//!
//! A saved instance holds every field — the initialiser writes them all — so
//! "observed wins" alone loses an edit to a field the game never touched. The
//! rule is a game save's three-way merge, per field: the snapshot records the
//! authored fields the instance arrived with (its base); a field whose observed
//! value still equals that base was left alone by the game, and takes what the
//! author says *now*. A field the game changed keeps the game's value.

use khora_core::script::{EventQueue, ScriptEvent, ScriptSnapshot, ScriptValue};
use khora_data::flow::{ScriptArrival, ScriptInstance, ScriptProgram, ScriptView};
use khora_script::arena::Persisted;
use khora_script::native::Host;

use super::{entity, runtime_of, MODULE};
use crate::script_lane::{run_behaviors, ScriptRuntime};

/// A guard with a field the game never changes and one it does.
const GUARD: &str = r#"
behavior Guard {
    int speed = 1;
    int health = 100;

    on Damaged(int amount) {
        health -= amount;
    }
}
"#;

fn authored(pairs: &[(&str, i64)]) -> Vec<(String, ScriptValue)> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), ScriptValue::Int(*value)))
        .collect()
}

/// The base a snapshot records: every field the layout declares, at its
/// override where the entity has one and at its declared default otherwise.
/// Sorted by name, so the comparison does not depend on declaration order.
fn base(pairs: &[(&str, i64)]) -> Vec<(String, ScriptValue)> {
    let mut base = authored(pairs);
    base.sort_by(|a, b| a.0.cmp(&b.0));
    base
}

/// A recorded base, sorted the way [`base`] sorts.
fn recorded_base(snapshot: &ScriptSnapshot) -> Vec<(String, ScriptValue)> {
    let mut base = snapshot.authored.clone();
    base.sort_by(|a, b| a.0.cmp(&b.0));
    base
}

fn view(arrival: Option<ScriptArrival>) -> ScriptView {
    ScriptView {
        resumed: false,
        delta_seconds: 0.0,
        input: Default::default(),
        programs: vec![ScriptProgram {
            module: MODULE.to_owned(),
            behavior: "Guard".to_owned(),
        }],
        instances: vec![ScriptInstance {
            entity: entity(0),
            program: 0,
            arrival,
            translation: khora_core::math::Vec3::ZERO,
            rotation: khora_core::math::Quaternion::IDENTITY,
            scale: khora_core::math::Vec3::ONE,
        }],
    }
}

/// One frame, returning what the lane wrote back for the scene.
fn frame(
    runtime: &mut ScriptRuntime,
    host: &mut Host,
    arrival: Option<ScriptArrival>,
    events: &EventQueue,
) -> ScriptSnapshot {
    let report = run_behaviors(&view(arrival), events, runtime, host, u64::MAX);
    report
        .state
        .first()
        .map(|update| update.snapshot.clone())
        .expect("the guard did work, so the lane recorded it")
}

/// The entity's first arrival in a fresh session, with `fields` authored and
/// `observed` from an earlier one. Returns the session and what its first
/// frame wrote back.
fn arrive(
    fields: Vec<(String, ScriptValue)>,
    observed: Option<ScriptSnapshot>,
    events: &EventQueue,
) -> (ScriptRuntime, Host, ScriptSnapshot) {
    let mut runtime = runtime_of(GUARD);
    let mut host = Host::new();
    let written = frame(
        &mut runtime,
        &mut host,
        Some(ScriptArrival { fields, observed }),
        events,
    );
    (runtime, host, written)
}

fn int_of(runtime: &ScriptRuntime, field: &str) -> Option<i64> {
    let slot = runtime.program(MODULE)?.layout("Guard")?.slot_of(field)?;
    match runtime.peek(entity(0), "Guard")?.fields.get(slot)? {
        Persisted::Scalar(value) => value.as_int(),
        _ => None,
    }
}

fn damaged(amount: i64) -> EventQueue {
    let mut queue = EventQueue::new();
    queue.push(ScriptEvent::new(entity(0), "Damaged").with(ScriptValue::Int(amount)));
    queue
}

/// **The edit this exists for.** The entity ran with `speed = 3` authored
/// and the game never touched it; the author then sets it to 5. Loading the
/// old snapshot against the edited scene gives 5.
#[test]
fn an_authored_edit_reaches_a_field_the_game_left_alone() {
    let (_, _, saved) = arrive(authored(&[("speed", 3)]), None, &EventQueue::new());
    assert_eq!(
        saved.field("speed"),
        Some(&ScriptValue::Int(3)),
        "the premise: the snapshot holds every field, speed included"
    );

    // The author edits `Script.fields`: speed 3 → 5.
    let (runtime, _, _) = arrive(authored(&[("speed", 5)]), Some(saved), &EventQueue::new());

    assert_eq!(
        int_of(&runtime, "speed"),
        Some(5),
        "the game left speed at what was authored, so the author's edit reaches it"
    );
}

/// **The other half of the rule.** A field the game changed keeps the game's
/// value whatever the author wrote since — and in the same arrival, a field it
/// left alone still takes the edit.
#[test]
fn a_field_the_game_changed_keeps_the_games_value() {
    let (_, _, saved) = arrive(
        authored(&[("speed", 3), ("health", 100)]),
        None,
        &damaged(60),
    );
    assert_eq!(
        saved.field("health"),
        Some(&ScriptValue::Int(40)),
        "the premise: the game hurt the guard"
    );

    // The author edits both: health 100 → 120, speed 3 → 5.
    let (runtime, _, _) = arrive(
        authored(&[("speed", 5), ("health", 120)]),
        Some(saved),
        &EventQueue::new(),
    );

    assert_eq!(
        (int_of(&runtime, "health"), int_of(&runtime, "speed")),
        (Some(40), Some(5)),
        "health keeps the game's 40; speed, never changed by the game, takes the edit"
    );
}

/// **A save from before the base was recorded.** With no `authored` in the
/// snapshot there is no base to tell "left alone" from "changed", so the
/// observed value wins, as it always did — and from that arrival on the
/// instance has a base, which its next snapshot records: the override where
/// there is one, the declared default elsewhere.
#[test]
fn an_old_snapshot_without_authored_keeps_todays_rule() {
    // Written by a build that had no `authored`: the key is absent.
    let mut old =
        serde_json::to_value(ScriptSnapshot::default().with_field("speed", ScriptValue::Int(3)))
            .expect("a snapshot serialises");
    old.as_object_mut()
        .expect("a snapshot is an object")
        .remove("authored");
    let old: ScriptSnapshot = serde_json::from_value(old).expect("an old snapshot still parses");
    assert!(old.authored.is_empty(), "the premise: no base is known");

    let (runtime, _, written) = arrive(authored(&[("speed", 5)]), Some(old), &EventQueue::new());

    assert_eq!(
        int_of(&runtime, "speed"),
        Some(3),
        "no base, so the observed value wins"
    );
    assert_eq!(
        recorded_base(&written),
        base(&[("speed", 5), ("health", 100)]),
        "the snapshot written after the arrival records the base it arrived with:          the speed override, and health at its declared default"
    );
}

/// **The base moves on.** After the merge, the next snapshot's `authored` is
/// the base as it is now — `Script.fields`' overrides over the declared
/// defaults — so a second edit is measured against the first, not against
/// the value the game first started from.
#[test]
fn the_authored_base_moves_on_with_the_next_save() {
    let (_, _, first) = arrive(authored(&[("speed", 3)]), None, &EventQueue::new());
    assert_eq!(
        recorded_base(&first),
        base(&[("speed", 3), ("health", 100)]),
        "the first snapshot records the base the entity arrived with"
    );

    let (_, _, second) = arrive(authored(&[("speed", 5)]), Some(first), &EventQueue::new());
    assert_eq!(
        recorded_base(&second),
        base(&[("speed", 5), ("health", 100)]),
        "the base is the one of the latest arrival"
    );
    assert_eq!(second.field("speed"), Some(&ScriptValue::Int(5)));

    // A second edit, 5 → 7, against the moved base.
    let (runtime, _, _) = arrive(authored(&[("speed", 7)]), Some(second), &EventQueue::new());
    assert_eq!(
        int_of(&runtime, "speed"),
        Some(7),
        "the game left speed at the moved base, so the second edit reaches it too"
    );
}

/// The base is the instance's, not the arrival frame's: every snapshot it
/// writes carries it, so a save taken many frames after the arrival still
/// knows what the observed values descend from.
#[test]
fn every_snapshot_carries_the_authored_base_not_only_the_first() {
    let (mut runtime, mut host, _) = arrive(authored(&[("speed", 3)]), None, &EventQueue::new());

    let later = frame(&mut runtime, &mut host, None, &damaged(10));

    assert_eq!(later.field("health"), Some(&ScriptValue::Int(90)));
    assert_eq!(
        recorded_base(&later),
        base(&[("speed", 3), ("health", 100)]),
        "a snapshot written after the arrival frame still records the base,          health at its declared default although the game has changed it since"
    );
}

/// **An override added since the save.** The guard ran with no override for
/// `speed`, so it held its declared default and the game never changed it.
/// The author then sets `speed = 5` on this entity: the game left the field
/// alone, so the override reaches it — the base for a field with no override
/// is the declared default, not "unknown".
#[test]
fn an_override_added_after_the_save_reaches_a_field_the_game_left_alone() {
    let (_, _, saved) = arrive(Vec::new(), None, &EventQueue::new());
    assert_eq!(
        saved.field("speed"),
        Some(&ScriptValue::Int(1)),
        "the premise: the snapshot holds the declared default"
    );

    let (runtime, _, _) = arrive(authored(&[("speed", 5)]), Some(saved), &EventQueue::new());

    assert_eq!(
        int_of(&runtime, "speed"),
        Some(5),
        "the game never changed speed, so the override added since reaches it"
    );
}

/// **An override removed since the save.** The guard ran with `speed = 3`
/// authored and the game never changed it. The author then removes the
/// override, returning the entity to the script's declared default: the game
/// left the field alone, so the declared default reaches it.
#[test]
fn an_override_removed_after_the_save_returns_an_untouched_field_to_its_default() {
    let (_, _, saved) = arrive(authored(&[("speed", 3)]), None, &EventQueue::new());
    assert_eq!(saved.field("speed"), Some(&ScriptValue::Int(3)));

    let (runtime, _, _) = arrive(Vec::new(), Some(saved), &EventQueue::new());

    assert_eq!(
        int_of(&runtime, "speed"),
        Some(1),
        "the game never changed speed, so removing the override returns it to the default"
    );
}

/// A guard that remembers where it first appeared.
const HOMING: &str = r#"
behavior Guard {
    Vec3 home = Position();
}
"#;

/// One frame of the homing guard standing at `x`, arriving with `observed`
/// when given.
fn homing_frame(
    runtime: &mut ScriptRuntime,
    x: f32,
    arrival: Option<ScriptArrival>,
) -> Option<ScriptSnapshot> {
    let mut view = view(arrival);
    view.instances[0].translation = khora_core::math::Vec3::new(x, 0.0, 0.0);
    let mut host = Host::new();
    let report = run_behaviors(&view, &EventQueue::new(), runtime, &mut host, u64::MAX);
    report.state.first().map(|update| update.snapshot.clone())
}

/// **A default that reads the world is what the game observed, not an
/// authored value to re-evaluate.** The guard appeared at x = 1 and recorded
/// it as `home`; the game then moved it to x = 5 and never wrote `home`. A
/// save and a load — no edit to anything — must give back the same `home`,
/// not wherever the guard stands when it is loaded.
#[test]
fn a_default_read_from_the_world_survives_a_save_unchanged() {
    let mut first = runtime_of(HOMING);
    let saved = homing_frame(
        &mut first,
        1.0,
        Some(ScriptArrival {
            fields: Vec::new(),
            observed: None,
        }),
    )
    .expect("the arrival ran the initialiser");
    let home = Some(&ScriptValue::Vec3(khora_core::math::Vec3::new(
        1.0, 0.0, 0.0,
    )));
    assert_eq!(saved.field("home"), home, "the premise: home is x = 1");

    // Loaded where the game had moved it to.
    let mut loaded = runtime_of(HOMING);
    homing_frame(
        &mut loaded,
        5.0,
        Some(ScriptArrival {
            fields: Vec::new(),
            observed: Some(saved),
        }),
    );
    let layout = loaded
        .program(MODULE)
        .and_then(|program| program.layout("Guard"))
        .expect("declared")
        .clone();
    let held = loaded.peek(entity(0), "Guard").map(|instance| {
        crate::script_lane::persistence::snapshot_from_store(&layout, &instance.fields)
    });

    assert_eq!(
        held.as_ref().and_then(|snapshot| snapshot.field("home")),
        home,
        "a save and a load, with no edit, leave `home` where the game had it"
    );
}

/// **A hot reload does not make a field look changed by the game.** The
/// author retunes `speed`'s default 1 → 2 while the game runs; the live guard
/// keeps its 1 (a reload keeps values). The game never touched `speed`, so a
/// later override the author adds must still reach it after a save and load —
/// exactly as it would have without the reload.
#[test]
fn a_reload_that_retunes_a_default_keeps_an_untouched_field_untouched() {
    const RETUNED: &str = r#"
behavior Guard {
    int speed = 2;
    int health = 100;

    on Damaged(int amount) {
        health -= amount;
    }
}
"#;
    let mut runtime = runtime_of(GUARD);
    let mut host = Host::new();
    frame(
        &mut runtime,
        &mut host,
        Some(ScriptArrival {
            fields: Vec::new(),
            observed: None,
        }),
        &EventQueue::new(),
    );
    runtime.reload(MODULE, super::compile(RETUNED));
    let saved = frame(&mut runtime, &mut host, None, &EventQueue::new());
    assert_eq!(
        saved.field("speed"),
        Some(&ScriptValue::Int(1)),
        "the premise: the reload kept the live value"
    );

    // The author adds an override; the game is loaded under the retuned script.
    let mut loaded = runtime_of(RETUNED);
    let mut host = Host::new();
    frame(
        &mut loaded,
        &mut host,
        Some(ScriptArrival {
            fields: authored(&[("speed", 5)]),
            observed: Some(saved),
        }),
        &EventQueue::new(),
    );

    assert_eq!(
        int_of(&loaded, "speed"),
        Some(5),
        "the game never changed speed, so the override reaches it"
    );
}
