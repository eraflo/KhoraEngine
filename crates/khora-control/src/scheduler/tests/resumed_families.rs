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

//! A family switched off and on again resumes on nothing lent. The frame
//! start that first finds it on again withdraws what its agent held — under
//! the agent's lock, before lending it anything — and lends it no allocation
//! taken up to then, not even one naming it: the envelope its previous
//! session was lent, or one decided while it was off, prices a world that
//! moved on. It runs at its own price until an allocation taken after its
//! resume lends it another. A family never seen off is never withdrawn.

use super::activation::{runtime_with, switch};
use super::allocations::{allocation, envelope, one_phase, scheduler_over};
use super::{offer_of, place_with, report_of, Doing};
use crate::agent_registry::AgentRegistry;
use crate::scheduler::*;
use khora_core::agent::gorna::protocol::{Envelope, Epoch, Offer, OfferRequest, Report};
use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::{Agent, ExecutionTiming, Place, Terms};
use khora_core::EngineContext;
use khora_data::ecs::World;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const SCRIPT: AgentId = AgentId::new("script");
const RENDERER: AgentId = AgentId::new("renderer");
const AUDIO: AgentId = AgentId::new("audio");

/// What an agent saw happen to it, in order.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Seen {
    /// `withdraw`: what it held was dropped.
    Withdrawn,
    /// `apply`: the epoch and envelope it was lent.
    Lent(Epoch, Envelope),
    /// A run of its layer, on the epoch it held then — `None` at its own
    /// price.
    Ran(Option<Epoch>),
}

type SharedLog = Arc<Mutex<Vec<(AgentId, Seen)>>>;

/// An agent that logs what it is lent and withdrawn, and the epoch its layer
/// runs on.
struct Resumer {
    id: AgentId,
    log: SharedLog,
    /// The epoch it holds, shared with its layer.
    held: Arc<Mutex<Option<Epoch>>>,
    place: Arc<Mutex<Place>>,
}

impl Resumer {
    fn new(id: AgentId, log: &SharedLog) -> Self {
        let held = Arc::new(Mutex::new(None));
        let run = {
            let (log, held) = (Arc::clone(log), Arc::clone(&held));
            move |_: &Terms, _: &mut EngineContext<'_>| {
                let epoch = *held.lock().expect("the held lock");
                log.lock()
                    .expect("the log lock")
                    .push((id, Seen::Ran(epoch)));
            }
        };
        Self {
            id,
            log: Arc::clone(log),
            held,
            place: place_with(id, Doing::new(StrategyId::Balanced, run), None),
        }
    }
}

impl Agent for Resumer {
    fn id(&self) -> AgentId {
        self.id
    }
    fn offer(&mut self, request: &OfferRequest) -> Offer {
        offer_of(self.id, request, StrategyId::Balanced)
    }
    fn report(&self) -> Report {
        report_of(self.id, StrategyId::Balanced)
    }
    fn apply(
        &mut self,
        epoch: Epoch,
        envelope: Option<&Envelope>,
        _pin: Option<StrategyId>,
        _allowed: Option<&[StrategyId]>,
    ) {
        *self.held.lock().expect("the held lock") = Some(epoch);
        self.log.lock().expect("the log lock").push((
            self.id,
            Seen::Lent(epoch, envelope.copied().unwrap_or_default()),
        ));
    }
    fn withdraw(&mut self) {
        *self.held.lock().expect("the held lock") = None;
        self.log
            .lock()
            .expect("the log lock")
            .push((self.id, Seen::Withdrawn));
    }
    fn place(&self) -> Arc<Mutex<Place>> {
        Arc::clone(&self.place)
    }
    fn execution_timing(&self) -> ExecutionTiming {
        one_phase(0.5)
    }
}

/// A scheduler over `ids`' agents, each a [`Resumer`] logging into `log`,
/// and the agents, in `ids`' order.
fn resumers(ids: &[AgentId], log: &SharedLog) -> (ExecutionScheduler, Vec<Arc<Mutex<Resumer>>>) {
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    let scheduler = scheduler_over(&registry);
    let agents: Vec<Arc<Mutex<Resumer>>> = ids
        .iter()
        .map(|id| Arc::new(Mutex::new(Resumer::new(*id, log))))
        .collect();
    {
        let mut registry = registry.lock().expect("the registry lock");
        for (rank, agent) in agents.iter().enumerate() {
            let erased: Arc<Mutex<dyn Agent>> = agent.clone();
            registry.register(erased, 1.0 + rank as f32);
        }
    }
    (scheduler, agents)
}

/// What `agent` saw, in order.
fn seen(log: &SharedLog, agent: AgentId) -> Vec<Seen> {
    log.lock()
        .expect("the log lock")
        .iter()
        .filter(|(id, _)| *id == agent)
        .map(|(_, seen)| *seen)
        .collect()
}

/// What `agent` was lent and withdrawn, in order, leaving its runs out.
fn handed(log: &SharedLog, agent: AgentId) -> Vec<Seen> {
    seen(log, agent)
        .into_iter()
        .filter(|seen| !matches!(seen, Seen::Ran(_)))
        .collect()
}

/// How many times `agent` was withdrawn.
fn withdrawals(log: &SharedLog, agent: AgentId) -> usize {
    seen(log, agent)
        .iter()
        .filter(|seen| **seen == Seen::Withdrawn)
        .count()
}

/// **A family on again is withdrawn once, at the frame start that finds it
/// on.** The script is lent epoch 1 and runs on it; switched off, it sits a
/// frame out; switched on, the frame start alone — before any frame runs —
/// withdraws what it held. Its next frames run at its own price, and it is
/// never withdrawn again while it stays on.
#[test]
fn a_family_on_again_is_withdrawn_once_at_the_frame_start_that_finds_it_on() {
    let log = SharedLog::default();
    let (mut scheduler, _agents) = resumers(&[SCRIPT], &log);
    let (runtime, activation) = runtime_with(&[SCRIPT]);
    let mut world = World::new();

    scheduler
        .budget_channel()
        .publish(allocation(1, &[("script", 2_000)]));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    switch(&activation, SCRIPT, false);
    scheduler.run_frame(&mut world, Arc::clone(&runtime));

    switch(&activation, SCRIPT, true);
    scheduler.begin_frame(&runtime);
    assert_eq!(
        seen(&log, SCRIPT),
        vec![
            Seen::Lent(Epoch(1), envelope(2_000)),
            Seen::Ran(Some(Epoch(1))),
            Seen::Withdrawn,
        ],
        "withdrawn at the frame start that finds it on again"
    );

    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(
        seen(&log, SCRIPT),
        vec![
            Seen::Lent(Epoch(1), envelope(2_000)),
            Seen::Ran(Some(Epoch(1))),
            Seen::Withdrawn,
            Seen::Ran(None),
            Seen::Ran(None),
        ],
        "withdrawn once; it then runs at its own price"
    );
}

/// **A family on again is lent nothing taken up to its resume.** The script
/// and the renderer are lent epoch 1. The script is switched off; a replayed
/// epoch 2, naming both, is taken while it is off; a live epoch 3, naming
/// both, is taken at the frame start that finds it on again. The script is
/// withdrawn there and lent neither — it runs at its own price — while the
/// renderer, on throughout, is lent each epoch in turn and never withdrawn.
#[test]
fn a_family_on_again_is_lent_no_allocation_taken_up_to_its_resume() {
    let log = SharedLog::default();
    let (mut scheduler, _agents) = resumers(&[SCRIPT, RENDERER], &log);
    let (runtime, activation) = runtime_with(&[SCRIPT, RENDERER]);
    let channel = scheduler.budget_channel().clone();
    let mut world = World::new();

    channel.publish(allocation(1, &[("script", 2_000), ("renderer", 3_000)]));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));

    switch(&activation, SCRIPT, false);
    channel.publish_in_turn(allocation(2, &[("script", 4_000), ("renderer", 5_000)]));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));

    switch(&activation, SCRIPT, true);
    channel.publish(allocation(3, &[("script", 6_000), ("renderer", 7_000)]));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));

    assert_eq!(
        seen(&log, SCRIPT),
        vec![
            Seen::Lent(Epoch(1), envelope(2_000)),
            Seen::Ran(Some(Epoch(1))),
            Seen::Withdrawn,
            Seen::Ran(None),
        ],
        "withdrawn, then lent neither the replayed epoch nor the one in force"
    );
    assert_eq!(
        handed(&log, RENDERER),
        vec![
            Seen::Lent(Epoch(1), envelope(3_000)),
            Seen::Lent(Epoch(2), envelope(5_000)),
            Seen::Lent(Epoch(3), envelope(7_000)),
        ],
        "a family on throughout is lent each epoch, and never withdrawn"
    );
}

/// **An allocation decided before a family went off is not lent when it
/// resumes.** The script runs at its own price; an allocation naming it is
/// published, and the script switched off before the frame start that takes
/// it. Switched on again, it is withdrawn and lent nothing: what was decided
/// for it before it went off is not lent after.
#[test]
fn an_allocation_decided_before_its_family_went_off_is_not_lent_when_it_resumes() {
    let log = SharedLog::default();
    let (mut scheduler, _agents) = resumers(&[SCRIPT], &log);
    let (runtime, activation) = runtime_with(&[SCRIPT]);
    let mut world = World::new();

    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    scheduler
        .budget_channel()
        .publish(allocation(1, &[("script", 2_000)]));
    switch(&activation, SCRIPT, false);
    scheduler.run_frame(&mut world, Arc::clone(&runtime));

    switch(&activation, SCRIPT, true);
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));

    assert_eq!(
        seen(&log, SCRIPT),
        vec![
            Seen::Ran(None),
            Seen::Withdrawn,
            Seen::Ran(None),
            Seen::Ran(None),
        ],
        "the allocation decided before it went off is never lent"
    );
}

/// **An allocation taken after a family resumed is lent as usual.** The
/// script, lent epoch 1, is switched off and on again, and withdrawn at its
/// resume. Epoch 2, published after that frame start, is lent at the next
/// one, and its layer runs on it.
#[test]
fn an_allocation_taken_after_a_family_resumed_is_lent_as_usual() {
    let log = SharedLog::default();
    let (mut scheduler, _agents) = resumers(&[SCRIPT], &log);
    let (runtime, activation) = runtime_with(&[SCRIPT]);
    let channel = scheduler.budget_channel().clone();
    let mut world = World::new();

    channel.publish(allocation(1, &[("script", 2_000)]));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    switch(&activation, SCRIPT, false);
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    switch(&activation, SCRIPT, true);
    scheduler.run_frame(&mut world, Arc::clone(&runtime));

    channel.publish(allocation(2, &[("script", 5_000)]));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));

    assert_eq!(
        seen(&log, SCRIPT),
        vec![
            Seen::Lent(Epoch(1), envelope(2_000)),
            Seen::Ran(Some(Epoch(1))),
            Seen::Withdrawn,
            Seen::Ran(None),
            Seen::Lent(Epoch(2), envelope(5_000)),
            Seen::Ran(Some(Epoch(2))),
        ]
    );
}

/// **An agent busy when its family resumes is withdrawn at the first frame
/// start it is free, before anything is lent to it.** The script is lent
/// epoch 1 and switched off; a replayed epoch 2 naming it is taken while it
/// is off. Switched on, its lock is held by another thread at that frame
/// start (the DCC asking it for an offer): it is neither withdrawn nor lent
/// there. Epoch 3 is published after; at the next frame start, free, it is
/// withdrawn first, never lent epoch 2, and lent epoch 3.
#[test]
fn an_agent_busy_when_its_family_resumes_is_withdrawn_at_the_first_frame_start_it_is_free() {
    let log = SharedLog::default();
    let (mut scheduler, agents) = resumers(&[SCRIPT], &log);
    let (runtime, activation) = runtime_with(&[SCRIPT]);
    let channel = scheduler.budget_channel().clone();
    let mut world = World::new();

    channel.publish(allocation(1, &[("script", 2_000)]));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    switch(&activation, SCRIPT, false);
    channel.publish_in_turn(allocation(2, &[("script", 4_000)]));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));

    switch(&activation, SCRIPT, true);
    let (holding, held) = std::sync::mpsc::channel::<()>();
    let (release, released) = std::sync::mpsc::channel::<()>();
    let busy = {
        let script = Arc::clone(&agents[0]);
        std::thread::spawn(move || {
            let _asked_for_an_offer = script.lock().expect("the agent lock");
            holding.send(()).expect("the test waits");
            let _ = released.recv_timeout(Duration::from_secs(5));
        })
    };
    held.recv().expect("the agent is busy");
    scheduler.begin_frame(&runtime);
    assert_eq!(
        handed(&log, SCRIPT),
        vec![Seen::Lent(Epoch(1), envelope(2_000))],
        "busy at its resume: neither withdrawn nor lent there"
    );
    release.send(()).expect("the busy thread waits");
    busy.join().expect("the busy thread");
    scheduler.run_frame(&mut world, Arc::clone(&runtime));

    channel.publish(allocation(3, &[("script", 6_000)]));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));

    assert_eq!(
        handed(&log, SCRIPT),
        vec![
            Seen::Lent(Epoch(1), envelope(2_000)),
            Seen::Withdrawn,
            Seen::Lent(Epoch(3), envelope(6_000)),
        ],
        "withdrawn once free, before anything is lent; epoch 2 never lent"
    );
    assert_eq!(seen(&log, SCRIPT).last(), Some(&Seen::Ran(Some(Epoch(3)))));
}

/// **Only a family seen off is withdrawn.** The renderer is on throughout;
/// the script is switched off and on again between two frame starts — the
/// scheduler never sees it off; the audio is seen off for a frame. Over the
/// frames that follow, the audio is withdrawn once, and neither the renderer
/// nor the script ever is.
#[test]
fn only_a_family_seen_off_is_withdrawn() {
    let log = SharedLog::default();
    let (mut scheduler, _agents) = resumers(&[RENDERER, SCRIPT, AUDIO], &log);
    let (runtime, activation) = runtime_with(&[RENDERER, SCRIPT, AUDIO]);
    let mut world = World::new();

    scheduler.budget_channel().publish(allocation(
        1,
        &[("renderer", 3_000), ("script", 2_000), ("audio", 1_000)],
    ));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));

    switch(&activation, SCRIPT, false);
    switch(&activation, SCRIPT, true);
    switch(&activation, AUDIO, false);
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    switch(&activation, AUDIO, true);
    for _ in 0..3 {
        scheduler.run_frame(&mut world, Arc::clone(&runtime));
    }

    assert_eq!(withdrawals(&log, AUDIO), 1, "seen off, then on again");
    assert_eq!(withdrawals(&log, RENDERER), 0, "on throughout");
    assert_eq!(
        withdrawals(&log, SCRIPT),
        0,
        "switched off and on again between two frame starts: never seen off"
    );
}
