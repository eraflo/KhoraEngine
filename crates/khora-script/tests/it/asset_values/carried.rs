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

//! An asset carried through a script: passed, held in a local, returned,
//! chosen between — the same identity all the way back to the engine.

use khora_core::script::{FrozenMachine, PendingBody, ScriptValue};
use khora_script::vm::{resume, Machine, ResumeTier, Run, Value};
use khora_script::Host;

use super::{asset, boundary, build, bullet_uuid, other_uuid, run_fn};

/// **An asset passed in, held in a local and returned comes back the same**
/// — the engine reads the identity it gave.
#[test]
fn an_asset_carried_through_a_local_and_a_return_is_the_same_asset() {
    let program = build(
        "fn Prefab? Keep(Prefab? p) { Prefab? held = p; return held; }
         fn Prefab? Through(Prefab? p) { return Keep(p); }",
    );
    for function in ["Keep", "Through"] {
        assert_eq!(
            boundary(run_fn(&program, function, &[asset(bullet_uuid())])),
            ScriptValue::Asset(bullet_uuid()),
            "`{function}`"
        );
        assert_eq!(
            run_fn(&program, function, &[Value::Null]),
            Value::Null,
            "`{function}` of null"
        );
    }
}

/// **A bare asset widens to its optional** where one is returned or held,
/// and a choice between two returns the one chosen.
#[test]
fn a_choice_between_two_assets_returns_the_one_chosen() {
    let program = build(
        "fn Sound? Pick(Sound a, Sound? b, bool first) {
             Sound? chosen = b;
             if (first) { chosen = a; }
             return chosen;
         }",
    );
    let (a, b) = (asset(bullet_uuid()), asset(other_uuid()));
    assert_eq!(
        boundary(run_fn(&program, "Pick", &[a, b, Value::Bool(true)])),
        ScriptValue::Asset(bullet_uuid())
    );
    assert_eq!(
        boundary(run_fn(&program, "Pick", &[a, b, Value::Bool(false)])),
        ScriptValue::Asset(other_uuid())
    );
    assert_eq!(
        run_fn(&program, "Pick", &[a, Value::Null, Value::Bool(false)]),
        Value::Null
    );
}

/// **An asset reaches the register and comes back unchanged** through the
/// engine's bridge, with no script in between.
#[test]
fn an_asset_crosses_the_bridge_unchanged() {
    assert_eq!(
        boundary(asset(bullet_uuid())),
        ScriptValue::Asset(bullet_uuid())
    );
}

/// Assets held in a local, an array and a struct field, compared after the
/// body is cut: `1111` when each still holds the asset it was given.
const HOLDS: &str = r#"struct Load { Prefab? p; int n = 0; }
fn int Score(Prefab? a, Prefab? b) {
    Prefab? x = a;
    Prefab?[] xs = [a, b, null];
    Load l = Load { p: b };
    int n = 0;
    int i = 0;
    while (i < 3) { i = i + 1; }
    if (x == a) { n = n + 1; }
    if (xs[1] == b) { n = n + 10; }
    if (xs[2] == null) { n = n + 100; }
    if (l.p == b) { n = n + 1000; }
    return n;
}"#;

/// **An asset held across a fuel cut is the same asset after a save** — in
/// a local, an array and a struct field, frozen, written out, read back and
/// thawed into a fresh frame, at every slice.
#[test]
fn an_asset_held_across_a_fuel_cut_survives_a_freeze() {
    let program = build(HOLDS);
    let args = [asset(bullet_uuid()), asset(other_uuid())];
    assert_eq!(
        run_fn(&program, "Score", &args),
        Value::Int(1111),
        "run whole"
    );
    for slice in 1..=120u64 {
        let mut machine = Machine::new(&program, "Score", &args).expect("`Score` exists");
        let mut host = Host::new();
        for _ in 0..10_000 {
            match machine.run(&program, &mut host, slice) {
                Run::Completed => break,
                Run::Suspended(_) => {
                    let frozen = machine
                        .freeze(&program, PendingBody::Sequence)
                        .unwrap_or_else(|| panic!("slice {slice}: freezes"));
                    let text = serde_json::to_string(&frozen).expect("writes");
                    let read: FrozenMachine = serde_json::from_str(&text).expect("reads");
                    host = Host::new();
                    machine = Machine::thaw(&read, &program)
                        .unwrap_or_else(|| panic!("slice {slice}: thaws"));
                }
                Run::Faulted(fault) => panic!("slice {slice}: {fault:?}"),
            }
        }
        assert_eq!(machine.result(), Value::Int(1111), "slice {slice}");
    }
}

/// `Guard.Hold` holding assets across an `await`; `bonus` is the edit.
fn guard(bonus: &str) -> String {
    format!(
        "struct Load {{ Prefab? p; int n = 0; }}
         behavior Guard {{
             async int Hold(Prefab? a, Prefab? b) {{
                 Prefab? x = a;
                 Prefab?[] xs = [a, b, null];
                 Load l = Load {{ p: b }};
                 await 1.0s;
                 int n = {bonus};
                 if (x == a) {{ n = n + 1; }}
                 if (xs[1] == b) {{ n = n + 10; }}
                 if (xs[2] == null) {{ n = n + 100; }}
                 if (l.p == b) {{ n = n + 1000; }}
                 return n;
             }}
         }}"
    )
}

/// **A body holding assets is rebuilt across an edit, not restarted** — its
/// local, array and struct field each still fit their declared asset type at
/// the new site, and read back the assets they held.
#[test]
fn a_body_holding_assets_is_rebuilt_across_an_edit() {
    let old = build(&guard("0"));
    let args = [asset(bullet_uuid()), asset(other_uuid())];
    let mut host = Host::new();
    let mut machine = Machine::new(&old, "Guard.Hold", &args).expect("`Guard.Hold` exists");
    assert!(
        matches!(machine.run(&old, &mut host, u64::MAX), Run::Suspended(_)),
        "stops at its await"
    );
    let frozen = machine
        .freeze(&old, PendingBody::Sequence)
        .expect("freezes");

    let edited = build(&guard("20000"));
    let (mut resumed, tier) = resume(&frozen, old.fingerprint(), &edited)
        .unwrap_or_else(|abandoned| panic!("resumes: {abandoned:?}"));
    assert_eq!(tier, ResumeTier::Rebuilt, "laid out again at its site");
    let mut host = Host::new();
    for _ in 0..1000 {
        match resumed.run(&edited, &mut host, u64::MAX) {
            Run::Completed => break,
            Run::Suspended(_) => host.awaiting = None,
            Run::Faulted(fault) => panic!("faulted: {fault:?}"),
        }
    }
    assert_eq!(resumed.result(), Value::Int(21111));
}

/// **An entity a body held is not rebuilt into an asset local**: the edit
/// that retypes the local `Prefab?` restarts the body — whose parameter
/// still takes the entity — rather than lay it out again with an entity
/// where an asset is declared.
#[test]
fn an_entity_held_is_not_rebuilt_into_an_asset_local() {
    let entity = Value::Entity(khora_core::ecs::entity::EntityId {
        index: 4,
        generation: 1,
    });
    let old = build(
        "behavior Guard {
             async int Hold(Entity? a) {
                 Entity? x = a;
                 await 1.0s;
                 int n = 0;
                 if (x == null) { n = 1; }
                 return n;
             }
         }",
    );
    let mut host = Host::new();
    let mut machine = Machine::new(&old, "Guard.Hold", &[entity]).expect("`Guard.Hold` exists");
    assert!(
        matches!(machine.run(&old, &mut host, u64::MAX), Run::Suspended(_)),
        "stops at its await"
    );
    let frozen = machine
        .freeze(&old, PendingBody::Sequence)
        .expect("freezes");

    let edited = build(
        "behavior Guard {
             async int Hold(Entity? a) {
                 Prefab? x = null;
                 await 1.0s;
                 int n = 10;
                 if (x == null) { n = 11; }
                 return n;
             }
         }",
    );
    let tier = resume(&frozen, old.fingerprint(), &edited).map(|(_, tier)| tier);
    assert_eq!(tier, Ok(ResumeTier::Restarted), "an entity is no prefab");
}
