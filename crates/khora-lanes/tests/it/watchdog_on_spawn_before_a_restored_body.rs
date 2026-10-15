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

//! **An `OnSpawn` the watchdog stops does not take the body a save restored
//! with it** (Ergon spec 10): a load owing `OnSpawn` runs it in front of the
//! body the save caught part-way. When that `OnSpawn` spins until the
//! watchdog faults it, the instance stops — and once an edit starts it again,
//! the restored body is the one owed next, as it is when `OnSpawn` finishes
//! or is abandoned.

use khora_core::script::{InstanceLifecycle, ScriptValue};

use super::watchdog_stage::{build, entity, event, recorded, Stage, MODULE, SLICE};

/// `OnSpawn` spins while `spin` is set; `Go` waits five seconds and marks
/// itself done through `Hold`.
const GUARD: &str = "behavior Guard {
                         bool spin = false;
                         int n = 0;
                         int done = 0;
                         void OnSpawn() { while (spin) { n += 1; } }
                         async void Hold() { await 5s; done = 1; }
                         on Go() { Hold(); }
                     }
                     event Go;";

/// The guard with its `OnSpawn` fixed — the edit that answers the fault.
const GUARD_FIXED: &str = "behavior Guard {
                               bool spin = false;
                               int n = 0;
                               int done = 0;
                               void OnSpawn() { n += 1; }
                               async void Hold() { await 5s; done = 1; }
                               on Go() { Hold(); }
                           }
                           event Go;";

/// Plenty of fuel for a turn that does not spin.
const PLENTY: u64 = 1_000_000;

#[test]
fn the_body_a_save_restored_resumes_after_its_on_spawn_was_stopped_and_fixed() {
    let subject = entity(0);

    // The running game: spawned, then `Go` caught waiting.
    let mut running = Stage::new(GUARD, &[(subject, "Guard")]);
    let report = running.frame_with(1.0, PLENTY, &event(subject, "Go"));
    let mut saved = recorded(&report, subject, "Guard");
    assert!(saved.pending.is_some(), "`Go` is part-way: {saved:?}");

    // As a save written before the lifecycle was recorded reads — `OnSpawn`
    // owed — with `spin` set, so the `OnSpawn` it owes never finishes.
    saved.lifecycle = InstanceLifecycle::default();
    for (name, value) in &mut saved.fields {
        if name == "spin" {
            *value = ScriptValue::Bool(true);
        }
    }
    if !saved.fields.iter().any(|(name, _)| name == "spin") {
        saved
            .fields
            .push(("spin".to_owned(), ScriptValue::Bool(true)));
    }

    let mut stage = Stage::loaded(GUARD, "Guard", saved);
    let stopped = stage.frames_until_disabled(subject, "Guard", 1.0, SLICE, 70);
    assert!(stopped.is_some(), "the spinning OnSpawn was stopped");
    assert_eq!(stage.int(subject, "Guard", "done"), Some(0));

    stage.runtime.reload(MODULE, build(GUARD_FIXED));
    stage.frames(10, 1.0, PLENTY);
    let instance = stage.instance(subject, "Guard");
    assert!(!instance.disabled, "the edit started it again");
    assert_eq!(
        stage.int(subject, "Guard", "done"),
        Some(1),
        "the `Go` the save restored finished after the edit; pending {:?}, after_load {:?}",
        instance.pending.as_ref().map(|pending| &pending.body),
        instance.after_load.as_ref().map(|pending| &pending.body),
    );
}
