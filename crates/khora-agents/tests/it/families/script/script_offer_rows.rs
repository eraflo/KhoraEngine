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

//! **The script family is priced on its measured demand.** Its lane quotes
//! two levels: the critical one, `LowPower`, at the lane's overhead and what
//! its `[Critical]` behaviors need, and the whole one, `Balanced`, at the
//! overhead and what every due instance needs — each turned into time at the
//! measured rate. A level's share says how many turns it serves on time — the
//! critical one the critical share of the due instances, the whole one all of
//! them — never what they compute, which the cuts leave exact. With free
//! frame time, GORNA lends the whole level.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use khora_control::gorna::{decide, Bid, Round};
use khora_core::agent::gorna::protocol::{
    CostBasis, Epoch, Offer, OfferRequest, OfferRow, Trigger,
};
use khora_core::agent::gorna::{AdaptationMode, AgentHints, AgentId, Planning, StrategyId};
use khora_core::agent::{Agent, AgentImportance};
use khora_core::lane::cost::{Cost, PerDevice, PerLink};
use khora_core::lane::OutputDeck;
use khora_data::ecs::{Transform, World};

use super::behavior_session::{attach, Project, Session};
use crate::families::script::fixtures::{
    bus_with, grant_row, price_of, run_script_agent, runtime_with, script_agent,
};

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

fn request() -> OfferRequest {
    OfferRequest {
        epoch: Epoch(1),
        target: TARGET,
        pressure: 0.0,
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

/// **Two levels on the measured demand: the critical one and the whole
/// one.** One `Vital` and three `Plain`s: the critical level is priced at the
/// overhead and the critical demand and serves one turn in four — the share
/// of the due instances that are critical; the whole level at the overhead
/// and the whole demand, and serves them all.
#[test]
fn the_script_lane_quotes_a_critical_level_and_a_whole_one_on_the_measured_demand() {
    let mut session = crew(1, 3);
    let quotes = session.quote();
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

    let ids: Vec<StrategyId> = quotes.iter().map(|(id, _)| *id).collect();
    assert_eq!(
        ids,
        [StrategyId::LowPower, StrategyId::Balanced],
        "{quotes:?}"
    );
    let critical = price_of(&quotes, StrategyId::LowPower);
    let whole = price_of(&quotes, StrategyId::Balanced);
    let floor = overhead + at_rate(demand.critical, rate);
    let all = overhead + at_rate(demand.all, rate);
    assert!(
        about(critical.time.cpu, floor),
        "the critical level is the overhead and the critical demand: {:?} vs {floor:?}",
        critical.time.cpu
    );
    assert!(
        about(whole.time.cpu, all),
        "the whole level is the overhead and the whole demand: {:?} vs {all:?}",
        whole.time.cpu
    );
    assert!(
        whole.time.cpu > overhead,
        "the whole level is above the overhead"
    );
    assert_eq!(
        critical.served(),
        0.25,
        "one of four due instances is critical"
    );
    assert_eq!(whole.served(), 1.0, "every turn on time");
}

/// **With no critical behavior, the critical level serves no turn on
/// time.** Four `Plain`s: the critical level is the overhead alone, serving
/// nothing.
#[test]
fn with_no_critical_behavior_the_critical_level_is_the_overhead_serving_nothing() {
    let mut session = crew(0, 4);
    let quotes = session.quote();
    let overhead = session.scripts.overhead();

    let critical = price_of(&quotes, StrategyId::LowPower);
    assert!(
        about(critical.time.cpu, overhead),
        "nothing critical to price past the overhead: {:?} vs {overhead:?}",
        critical.time.cpu
    );
    assert_eq!(critical.served(), 0.0, "no due instance is critical");
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
        confidence: CostBasis::Measured,
    }
}

/// **With free frame time, GORNA lends the script family its whole level.**
/// Four `Plain`s, none critical, so the critical level serves no turn on
/// time. The family's agent runs a frame on its critical level, so both
/// levels are priced — that one measured, the whole one quoted — and bids
/// beside a renderer whose dearer row takes half the frame. The round
/// upgrades the script to its whole level, the envelope it lends covers that
/// level, and the agent runs it from the next frame.
#[test]
fn with_free_frame_time_gorna_lends_the_script_family_its_whole_level() {
    let mut session = crew(0, 4);
    let view = session.last_view();
    let shared = Arc::new(Mutex::new(std::mem::take(&mut session.scripts)));
    let runtime = Arc::new(runtime_with(&shared));
    let mut agent = script_agent();
    grant_row(&mut agent, StrategyId::LowPower, TARGET);
    run_script_agent(
        &mut agent,
        &runtime,
        &bus_with(view.clone()),
        &mut OutputDeck::new(),
    );

    let offer = agent.offer(&request());
    let floor = row(&offer, StrategyId::LowPower);
    let whole = row(&offer, StrategyId::Balanced);
    assert!(
        floor.cost.time.cpu < whole.cost.time.cpu,
        "set-up: the critical level {:?} is cheaper than the whole one {:?}",
        floor.cost.time.cpu,
        whole.cost.time.cpu
    );
    assert_eq!(
        (floor.confidence, whole.confidence),
        (CostBasis::Measured, CostBasis::Quoted),
        "the level that ran is measured, the other quoted"
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
        background: &[],
        bids: &bids,
        planning: Planning::default(),
    });
    let envelope = allocation
        .envelopes
        .iter()
        .find(|(id, _)| *id == agent.id())
        .map(|(_, envelope)| envelope.time.cpu)
        .expect("the script family's agent is lent an envelope");
    assert!(
        allocation.lend_to(&mut agent),
        "the allocation names the agent"
    );
    run_script_agent(
        &mut agent,
        &runtime,
        &bus_with(view),
        &mut OutputDeck::new(),
    );

    assert!(
        envelope >= whole.cost.time.cpu,
        "the envelope {envelope:?} covers the whole level {:?}",
        whole.cost.time.cpu
    );
    let installed = agent
        .place()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .installed_row();
    assert_eq!(
        installed,
        Some(StrategyId::Balanced),
        "the agent runs its whole level"
    );
}
