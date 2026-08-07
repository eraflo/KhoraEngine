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

//! Folding this frame's OS input into the action map.
//!
//! [`InputMap`] is a **derived view** over
//! [`Channel<InputEvent>`](khora_core::event::Channel): the channel is what
//! happened, the map is the answer to "is jump held". Deriving it is a tick
//! invariant like any other, so it is a `DataSystem` and not a call somewhere
//! in the engine's frame method — `RULES.md` §3: *engine-tick wiring is
//! data-driven; never wire a system manually in engine code.*
//!
//! # Why `PreSimulation`
//!
//! The phase's own documentation asks for exactly this — *"input-driven
//! mutations, scene events, anything that should be visible to agents at the
//! start of their frame"* — and until this system it had no production
//! registration at all. An agent that reads the map reads it after this ran,
//! which is the whole point of the phase existing.
//!
//! # Why it reads rather than drains
//!
//! Its cursor is its own. The app-facing `update(world, inputs)` hook reads the
//! same stream under another name, and a future agent will read it under a
//! third; a drain here would empty it for whichever of them ran second. That is
//! the bug the channel's per-reader cursors exist to prevent, and it would be a
//! quiet one — input that works until the day two things want it.

use std::sync::{Arc, Mutex};

use khora_core::event::Channel;
use khora_core::lane::OutputDeck;
use khora_core::platform::{InputEvent, InputMap};
use khora_core::Runtime;

use crate::ecs::{DataSystemRegistration, TickPhase, World};

fn input_map_update(_world: &mut World, runtime: &Runtime, _deck: &mut OutputDeck) {
    let Some(events) = runtime.resources.get::<Channel<InputEvent>>() else {
        // A headless host, or a test harness with no window. Nothing produces
        // input, so there is nothing to derive.
        return;
    };
    let Some(map) = runtime.resources.get::<Arc<Mutex<InputMap>>>() else {
        return;
    };

    // Read even when the map is unreachable would be wrong, and read even when
    // there is nothing would be pointless: the cursor is only advanced once
    // there is somewhere to put the result.
    let this_frame = events.read_for("input_map");
    match map.lock() {
        Ok(mut map) => map.update(&this_frame),
        Err(_) => log::error!("the input map is poisoned; this frame's input is not applied"),
    }
}

inventory::submit! {
    DataSystemRegistration {
        name: "input_map_update",
        phase: TickPhase::PreSimulation,
        run: input_map_update,
        // First in the phase. Anything else registered here reacts to input —
        // that is what the phase is for — and reacting to the previous frame's
        // held actions is a frame of lag nobody asked for.
        order_hint: -100,
        runs_after: &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::platform::{input_channel, InputBinding, KeyCode};

    /// A runtime holding both halves, and the channel handle to write through.
    fn wired() -> (Runtime, Channel<InputEvent>, Arc<Mutex<InputMap>>) {
        let mut map = InputMap::new();
        map.bind("jump", InputBinding::Key(KeyCode::Space));
        let map = Arc::new(Mutex::new(map));
        let events = input_channel();

        let mut runtime = Runtime::default();
        runtime.resources.insert(events.clone());
        runtime.resources.insert(Arc::clone(&map));
        (runtime, events, map)
    }

    fn run(runtime: &Runtime) {
        input_map_update(&mut World::new(), runtime, &mut OutputDeck::new());
    }

    #[test]
    fn a_press_reaches_the_map() {
        let (runtime, events, map) = wired();
        events.send(InputEvent::KeyPressed {
            key_code: KeyCode::Space,
        });

        run(&runtime);

        assert!(map.lock().expect("not poisoned").is_pressed("jump"));
    }

    /// **The reason it reads rather than drains.** The app hook and any agent
    /// read the same stream under their own names; a drain here would empty it
    /// for whichever ran second.
    #[test]
    fn what_it_read_is_still_there_for_another_reader() {
        let (runtime, events, _map) = wired();
        events.send(InputEvent::KeyPressed {
            key_code: KeyCode::Space,
        });

        run(&runtime);

        assert_eq!(
            events.read_for("somebody_else").len(),
            1,
            "the map derived from it; it did not consume it"
        );
    }

    /// Its cursor is its own, so a second frame folds only the second frame.
    #[test]
    fn a_second_run_sees_only_what_arrived_since() {
        let (runtime, events, map) = wired();
        events.send(InputEvent::KeyPressed {
            key_code: KeyCode::Space,
        });
        run(&runtime);

        // Nothing new. `just_pressed` is an edge, so it must not fire twice for
        // one press — which it would if the cursor had not moved.
        run(&runtime);

        assert!(map.lock().expect("not poisoned").is_pressed("jump"));
        assert!(!map.lock().expect("not poisoned").just_pressed("jump"));
    }

    /// A headless host has neither, and that is the shipping path for a server
    /// build rather than a failure.
    #[test]
    fn a_runtime_with_no_input_runs_without_complaint() {
        run(&Runtime::default());
    }
}
