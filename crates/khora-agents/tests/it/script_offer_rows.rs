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

//! **The script agent is priced on its measured demand.** It offers two
//! rows: a floor, `LowPower`, of the lane's overhead and what its
//! `[Critical]` behaviors need, and a full row, `Balanced`, of the overhead
//! and what every due instance needs — each turned into time at the measured
//! rate, both `Measured` once a full turn is known. A row's quality says how
//! many turns it serves on time — the floor the critical share of the due
//! instances, the full row all of them — never what they compute, which the
//! cuts leave exact. With free frame time, GORNA lends the full row.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use khora_agents::script_agent::ScriptAgent;
use khora_control::gorna::{decide, Bid, Round};
use khora_core::agent::gorna::protocol::{
    Confidence, Epoch, Offer, OfferRequest, OfferRow, Trigger,
};
use khora_core::agent::gorna::{AdaptationMode, AgentHints, AgentId, StrategyId};
use khora_core::agent::{Agent, AgentImportance, EngineMode};
use khora_core::lane::cost::{Cost, PerDevice, PerLink};
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::{EngineContext, Runtime, WorldAccess};
use khora_data::ecs::{Transform, World};

use super::behavior_session::{attach, Project, Session};

/// The frame offered for: 1/60 s.
const TARGET: Duration = Duration::from_micros(16_667);

/// `Vital`, critical, and `Plain`, each turn a loop.
const CREW: Project = &[(
    "game/crew.erg",
    "[Critical]
     behavior Vital { int n = 0; void Update(float dt) { int i = 0; while (i < 100) { i += 1; } n += 1; } }
     behavior Plain { int n = 0; void Update(float dt) { int i = 0; while (i < 300) { i += 1; } n += 1; } }",
)];

/// A session of `vital` `Vital`s and `plain` `Plain`s, three frames played.
fn crew(vital: usize, plain: usize) -> Session {
    let mut session = Session::new(World::new(), CREW);
    for (behavior, count) in [("Vital", vital), ("Plain", plain)] {
        for _ in 0..count {
            let entity = session.world.spawn(Transform::identity());
            attach(&mut session.world, entity, behavior, &[]);
        }
    }
    for _ in 0..3 {
        session.frame(TARGET.as_secs_f32());
    }
    session
}

/// `read` of a script agent handed the session's scripting world, as the
/// engine hands it; the world goes back to the session after.
fn with_agent<T>(session: &mut Session, read: impl FnOnce(&mut ScriptAgent) -> T) -> T {
    let shared = Arc::new(Mutex::new(std::mem::take(&mut session.scripts)));
    let answer = {
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
        read(&mut agent)
    };
    session.scripts = Arc::try_unwrap(shared)
        .expect("the agent is gone, and its handle with it")
        .into_inner()
        .expect("the scripting world's lock is not poisoned");
    answer
}

fn request() -> OfferRequest {
    OfferRequest {
        epoch: Epoch(1),
        target: TARGET,
        pressure: 0.0,
        mode: EngineMode::Playing,
        reply_by: Instant::now() + Duration::from_secs(1),
    }
}

/// The row `id` of `offer`.
fn row(offer: &Offer, id: StrategyId) -> OfferRow {
    *offer
        .rows
        .iter()
        .find(|row| row.id == id)
        .unwrap_or_else(|| panic!("a {id:?} row in {offer:?}"))
}

/// The time `fuel` takes at `rate` instructions per millisecond.
fn at_rate(fuel: u64, rate: f64) -> Duration {
    Duration::from_secs_f64(fuel as f64 / rate / 1_000.0)
}

/// Whether `actual` is `expected` within a microsecond.
fn about(actual: Duration, expected: Duration) -> bool {
    actual.abs_diff(expected) <= Duration::from_micros(1)
}

/// **Two measured rows: the critical floor and the full demand.** One
/// `Vital` and three `Plain`s: the floor is `LowPower`, priced at the
/// overhead and the critical demand, of quality one in four — the share of
/// the due instances that are critical; the full row, `Balanced`, at the
/// overhead and the whole demand, of quality one. Both are measured.
#[test]
fn the_script_agent_offers_a_critical_floor_and_a_full_row_both_measured() {
    let mut session = crew(1, 3);
    let offer = with_agent(&mut session, |agent| agent.offer(&request()));
    let scripts = &session.scripts;
    let (overhead, rate) = (scripts.overhead(), scripts.rate());
    let demand = scripts.demand(TARGET.as_secs_f32());
    assert_eq!(
        scripts.last_report().swept_rows,
        0,
        "set-up: no system swept"
    );
    assert!(
        overhead > Duration::ZERO,
        "set-up: the lane measured its overhead"
    );
    assert!(
        demand.critical > 0 && demand.critical < demand.all,
        "set-up: {demand:?}"
    );

    let ids: Vec<StrategyId> = offer.rows.iter().map(|row| row.id).collect();
    assert_eq!(
        ids,
        [StrategyId::LowPower, StrategyId::Balanced],
        "{offer:?}"
    );
    assert_eq!(offer.floor, StrategyId::LowPower);

    let floor = row(&offer, StrategyId::LowPower);
    let full = row(&offer, StrategyId::Balanced);
    let critical = overhead + at_rate(demand.critical, rate);
    let all = overhead + at_rate(demand.all, rate);
    assert!(
        about(floor.cost.time.cpu, critical),
        "the floor is the overhead and the critical demand: {:?} vs {critical:?}",
        floor.cost.time.cpu
    );
    assert!(
        about(full.cost.time.cpu, all),
        "the full row is the overhead and the whole demand: {:?} vs {all:?}",
        full.cost.time.cpu
    );
    assert!(
        full.cost.time.cpu > overhead,
        "the full row is above the overhead"
    );
    assert_eq!(floor.quality, 0.25, "one of four due instances is critical");
    assert_eq!(full.quality, 1.0, "every turn on time");
    assert_eq!(
        (floor.confidence, full.confidence),
        (Confidence::Measured, Confidence::Measured)
    );
}

/// **With no critical behavior, the floor serves no turn on time.** Four
/// `Plain`s: the floor is the overhead alone, of quality zero.
#[test]
fn with_no_critical_behavior_the_floor_is_the_overhead_of_quality_zero() {
    let mut session = crew(0, 4);
    let offer = with_agent(&mut session, |agent| agent.offer(&request()));
    let overhead = session.scripts.overhead();

    let floor = row(&offer, StrategyId::LowPower);
    assert!(
        about(floor.cost.time.cpu, overhead),
        "nothing critical to price past the overhead: {:?} vs {overhead:?}",
        floor.cost.time.cpu
    );
    assert_eq!(floor.quality, 0.0, "no due instance is critical");
}

/// A bid as GORNA weighs one, under no policy, holding nothing.
fn bid(offer: Offer, importance: AgentImportance) -> Bid {
    Bid {
        offer,
        importance,
        mode: AdaptationMode::Learning,
        hints: AgentHints::default(),
        kept: None,
        late: false,
    }
}

/// A row of `micros` on the host and no memory.
fn on_cpu(id: StrategyId, micros: u64, quality: f32) -> OfferRow {
    OfferRow {
        id,
        cost: Cost {
            time: PerDevice {
                cpu: Duration::from_micros(micros),
                ..PerDevice::default()
            },
            ..Cost::default()
        },
        quality,
        confidence: Confidence::Measured,
    }
}

/// **With free frame time, GORNA lends the full row.** Four `Plain`s, none
/// critical, so the floor serves no turn on time; the script agent bids
/// beside a renderer whose dearer row takes half the frame. The round
/// upgrades the script to its full row — its quality is worth the few
/// microseconds — and the envelope it lends covers that row, which the agent
/// then runs. Lent its floor and a share of the slack in proportion to it,
/// the script would run short of its full demand.
#[test]
fn with_free_frame_time_gorna_lends_the_script_its_full_row() {
    let mut session = crew(0, 4);
    let (full, envelope, running) = with_agent(&mut session, |agent| {
        let offer = agent.offer(&request());
        let floor = row(&offer, StrategyId::LowPower).cost.time.cpu;
        let full = row(&offer, StrategyId::Balanced).cost.time.cpu;
        assert!(
            floor < full,
            "set-up: the floor {floor:?} is cheaper than the full row {full:?}"
        );
        let renderer = Offer {
            agent: AgentId::new("renderer"),
            epoch: Epoch(1),
            rows: vec![
                on_cpu(StrategyId::LowPower, 2_000, 0.4),
                on_cpu(StrategyId::Balanced, 8_000, 1.0),
            ],
            floor: StrategyId::LowPower,
            current: StrategyId::LowPower,
            health: 1.0,
        };
        let bids = [
            bid(offer, AgentImportance::Critical),
            bid(renderer, AgentImportance::Important),
        ];
        let allocation = decide(&Round {
            epoch: Epoch(1),
            trigger: Trigger::Refresh,
            target: TARGET,
            memory: PerDevice::default(),
            link_rate: PerLink::default(),
            waves: &[],
            bids: &bids,
        });
        let envelope = allocation
            .envelopes
            .iter()
            .find(|(id, _)| *id == agent.id())
            .map(|(_, envelope)| envelope.time.cpu)
            .expect("the script agent is lent an envelope");
        assert!(allocation.lend_to(agent), "the allocation names the agent");
        (full, envelope, agent.report_status().current_strategy)
    });

    assert!(
        envelope.as_micros() >= full.as_micros(),
        "the envelope {envelope:?} covers the full row {full:?}"
    );
    assert_eq!(running, StrategyId::Balanced, "the agent runs its full row");
}
