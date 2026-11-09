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

//! The order a phase's agents run in: by importance, then by priority —
//! higher first — wherever no dependency says otherwise.

use super::{offer_of, report_of};
use crate::agent_registry::AgentRegistry;
use crate::dcc_context::Context;
use crate::scheduler::*;
use khora_core::agent::gorna::protocol::{Envelope, Epoch, Offer, OfferRequest, Report};
use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::{Agent, ExecutionPhase, ExecutionTiming};
use khora_core::{EngineContext, Runtime};
use khora_data::ecs::World;
use std::sync::{Arc, Mutex, RwLock};

/// An agent of `OUTPUT` alone, at a priority, writing its name when it runs.
struct Ranked {
    id: AgentId,
    priority: f32,
    ran: Arc<Mutex<Vec<AgentId>>>,
}

impl Agent for Ranked {
    fn id(&self) -> AgentId {
        self.id
    }
    fn offer(&mut self, request: &OfferRequest) -> Offer {
        offer_of(self.id, request, StrategyId::Balanced)
    }
    fn apply(
        &mut self,
        _epoch: Epoch,
        _envelope: &Envelope,
        _pin: Option<StrategyId>,
        _allowed: Option<&[StrategyId]>,
    ) {
    }
    fn report(&self) -> Report {
        report_of(self.id, StrategyId::Balanced)
    }
    fn execution_timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            allowed_phases: vec![ExecutionPhase::OUTPUT],
            default_phase: ExecutionPhase::OUTPUT,
            priority: self.priority,
            ..ExecutionTiming::default()
        }
    }
    fn execute(&mut self, _ctx: &mut EngineContext<'_>) {
        self.ran.lock().expect("the run log lock").push(self.id);
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// **Agents of one phase run in priority order**, highest first, every frame.
/// Three independent agents of one importance in `OUTPUT`, registered in an
/// order that is not theirs (`middle`, `low`, `high`): every frame runs
/// `high`, `middle`, `low` — over ten fresh schedulers of five frames each,
/// so an order that came from hashing their names would show.
#[test]
fn agents_of_one_phase_run_in_priority_order() {
    let expected = [
        AgentId::new("high"),
        AgentId::new("middle"),
        AgentId::new("low"),
    ];
    let mut frames = Vec::new();
    for _ in 0..10 {
        let ran = Arc::new(Mutex::new(Vec::new()));
        let registry = Arc::new(Mutex::new(AgentRegistry::new()));
        {
            let mut registry = registry.lock().expect("the registry lock");
            for (name, priority) in [("middle", 2.0), ("low", 1.0), ("high", 3.0)] {
                registry.register(
                    Arc::new(Mutex::new(Ranked {
                        id: AgentId::new(name),
                        priority,
                        ran: Arc::clone(&ran),
                    })),
                    priority,
                );
            }
        }
        let mut scheduler =
            ExecutionScheduler::from_registry(registry, Arc::new(RwLock::new(Context::default())));
        let mut world = World::new();
        let runtime = Arc::new(Runtime::new());
        for _ in 0..5 {
            scheduler.run_frame(&mut world, Arc::clone(&runtime));
            frames.push(std::mem::take(&mut *ran.lock().expect("the run log lock")));
        }
    }

    let out_of_order: Vec<&Vec<AgentId>> = frames
        .iter()
        .filter(|frame| frame.as_slice() != expected)
        .collect();
    assert!(
        out_of_order.is_empty(),
        "{} of {} frames ran out of priority order, e.g. {:?}",
        out_of_order.len(),
        frames.len(),
        out_of_order.first()
    );
}
