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

//! **GORNA prices the rows a system sweeps** (Ergon spec 10): one system is
//! one instance whatever its queries match, so the script agent's full row
//! counts the rows the systems' queries took last frame on top of the lane's
//! overhead and the measured demand — ten thousand rows are not the cost of
//! one behavior.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use khora_agents::script_agent::ScriptAgent;
use khora_core::agent::gorna::{NegotiationRequest, ResourceConstraints, StrategyId};
use khora_core::agent::{Agent, ExecutionTiming};
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::script::ScriptValue;
use khora_core::{EngineContext, Runtime, WorldAccess};
use khora_data::ecs::{Transform, World};
use khora_lanes::script_lane::ScriptRuntime;

use super::behavior_session::{attach, Project};
use super::system_session::SystemSession;

const CENSUS: Project = &[
    (
        "game/health.erg",
        "component Health { int current = 0; int max = 10; }",
    ),
    (
        "game/census.erg",
        "import \"game/health.erg\";
         system Census {
             int total = 0;
             void Update(float dt) { total = 0; foreach (Health h) { total += h.current; } }
         }",
    ),
];

/// What the Balanced estimate the script agent offers, once a session over
/// `rows` entities holding `Health` has played two frames, prices on top of
/// the lane's measured overhead and the measured demand at the measured rate:
/// the rows the system swept.
fn rows_priced(rows: i64) -> Duration {
    let mut session = SystemSession::new(World::new(), CENSUS);
    for current in 0..rows {
        let entity = session.world.spawn(Transform::identity());
        attach(
            &mut session.world,
            entity,
            "Health",
            &[("current", ScriptValue::Int(current))],
        );
    }
    session.frame(0.016);
    session.frame(0.016);

    let shared = Arc::new(Mutex::new(std::mem::replace(
        &mut session.scripts,
        ScriptRuntime::new(),
    )));
    let mut runtime = Runtime::default();
    runtime.services.insert(Arc::clone(&shared));
    let runtime = Arc::new(runtime);

    let mut agent = ScriptAgent::default();
    let bus = LaneBus::new();
    let mut deck = OutputDeck::new();
    let permit = agent.contention();
    {
        let mut ctx = EngineContext::for_agent(
            WorldAccess::None,
            Arc::clone(&runtime),
            &bus,
            &mut deck,
            &permit,
            Some(agent.id()),
        );
        agent.on_initialize(&mut ctx);
    }
    let target = Duration::from_millis(16);
    let response = agent.negotiate(NegotiationRequest {
        target_latency: target,
        priority_weight: 1.0,
        constraints: ResourceConstraints::default(),
        agent_timing: ExecutionTiming::default(),
    });
    let balanced = response
        .strategies
        .iter()
        .find(|option| option.id == StrategyId::Balanced)
        .map(|option| option.estimated_time)
        .expect("a Balanced offer");
    let scripts = shared.lock().expect("the scripting world's lock is free");
    let demand = scripts.demand(target.as_secs_f32()).all;
    let work = Duration::from_secs_f64(demand as f64 / scripts.rate() / 1_000.0);
    balanced.saturating_sub(scripts.overhead() + work)
}

/// **The estimate grows with the rows swept**: the same one system over a
/// hundred times the rows is priced higher past its overhead and its
/// measured turn — and it ran as one instance in both.
#[test]
fn the_script_agents_estimate_grows_with_the_rows_its_systems_swept() {
    let few = rows_priced(4);
    let many = rows_priced(400);

    assert!(
        many > few,
        "400 rows swept are priced above 4: {many:?} vs {few:?}"
    );
}
