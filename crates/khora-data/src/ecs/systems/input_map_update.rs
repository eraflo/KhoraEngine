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

//! Folds this frame's input from the
//! [`Channel<InputEvent>`](khora_core::event::Channel) into the [`InputMap`].
//!
//! It reads under its own cursor rather than draining the channel: the app's
//! `update` hook reads the same stream, and a drain would empty it for
//! whichever ran second.

use crate::ecs::SystemContext;
use std::sync::{Arc, Mutex};

use khora_core::event::Channel;
use khora_core::lane::OutputDeck;
use khora_core::platform::{InputEvent, InputMap};
use khora_core::Runtime;

use crate::ecs::{DataSystemRegistration, TickPhase, World};

fn input_map_update(
    _world: &mut World,
    runtime: &Runtime,
    _deck: &mut OutputDeck,
    _ctx: SystemContext,
) {
    let Some(events) = runtime.resources.get::<Channel<InputEvent>>() else {
        // A headless host or a test harness: nothing produces input.
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
        // First in the phase: what else runs here reacts to input, and reading
        // the previous frame's actions would be a frame of lag.
        order_hint: -100,
        runs_after: &[],
        observes: &[],
        role: crate::ecs::Role::Upkeep,
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
        input_map_update(
            &mut World::new(),
            runtime,
            &mut OutputDeck::new(),
            crate::ecs::SystemContext {
                since: crate::ecs::Tick(0),
                resumed: false,
                scope: None,
            },
        );
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

    /// **It reads rather than drains**, so another reader of the stream still
    /// sees this frame's input.
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
