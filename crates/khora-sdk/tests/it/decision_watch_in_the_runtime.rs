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

//! The DCC's last decision, reached by a tool through the runtime as it
//! reaches the agent registry and the DCC's context.

use std::time::Duration;

use khora_sdk::khora_core::agent::gorna::protocol::{Epoch, Trigger};
use khora_sdk::khora_core::agent::gorna::{DecisionView, DecisionWatch};
use khora_sdk::{
    EngineApp, EngineCore, GameWorld, InputEvent, PhaseProvider, Runtime, WindowConfig,
};

/// An application that does nothing: the bootstrap alone is under test.
struct Idle;

impl PhaseProvider for Idle {}
impl EngineApp for Idle {
    fn window_config() -> WindowConfig {
        WindowConfig::default()
    }
    fn new() -> Self {
        Idle
    }
    fn setup(&mut self, _world: &mut GameWorld, _runtime: &Runtime) {}
    fn update(&mut self, _world: &mut GameWorld, _inputs: &[InputEvent]) {}
}

/// A decision no round of the engine would publish: of epoch `epoch`, far
/// beyond any the DCC opens in a test, opened by a policy change and replayed.
fn a_marked_decision(epoch: u64) -> DecisionView {
    DecisionView {
        epoch: Epoch(epoch),
        trigger: Trigger::PolicyChanged,
        target: Duration::from_micros(12_345),
        replayed: true,
        ..DecisionView::default()
    }
}

/// Whether a decision published through `from` is read back through `to`.
/// The DCC's own thread may publish between the two, so the publish is tried
/// a few times: two handles on one decision read it back at once, two
/// separate ones never do.
fn shared(from: &DecisionWatch, to: &DecisionWatch, epoch: u64) -> bool {
    (0..20).any(|_| {
        from.publish(a_marked_decision(epoch));
        to.latest() == Some(a_marked_decision(epoch))
    })
}

/// **The engine puts the decision watch in the runtime.** After the
/// bootstrap, the runtime's resources hold a `DecisionWatch` beside the agent
/// registry and the DCC's context, and it is the DCC's own: a decision the
/// DCC's handle publishes is read through the resource, and the reverse.
#[test]
fn the_engine_puts_the_decision_watch_in_the_runtime() {
    let mut engine = EngineCore::<Idle>::new();
    engine.bootstrap(Idle, Runtime::new());

    let in_the_runtime = engine
        .runtime()
        .resources
        .get::<DecisionWatch>()
        .cloned()
        .expect("the bootstrap puts the DCC's decision watch in the runtime");
    let dcc = engine
        .dcc()
        .expect("the bootstrap starts the DCC")
        .decision_watch();

    assert!(
        shared(&dcc, &in_the_runtime, 9_000_001),
        "a decision the DCC publishes is read through the runtime"
    );
    assert!(
        shared(&in_the_runtime, &dcc, 9_000_002),
        "one handle, read from either side"
    );
}
