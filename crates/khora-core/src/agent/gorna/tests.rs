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

//! An agent written before the protocol answers it through the trait's
//! bridge defaults: its `negotiate` options become offer rows, an envelope
//! becomes the budget of the dearest option that fits it, and its status
//! becomes a report.

use std::time::{Duration, Instant};

use super::protocol::{
    Allocation, Confidence, Envelope, Epoch, Offer, OfferRequest, OfferRow, Report, Trigger,
};
use super::{
    AdaptationMode, AgentHints, AgentId, AgentStatus, DecisionTrace, EpochRecord,
    NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId, StrategyOption,
};
use crate::agent::{Agent, EngineMode};
use crate::lane::cost::{Cost, PerDevice};
use crate::EngineContext;

/// An agent implementing only the old negotiation: three options, the
/// budgets it was applied, and the requests it was asked with.
struct Old {
    options: Vec<StrategyOption>,
    applied: Vec<ResourceBudget>,
    asked: Vec<NegotiationRequest>,
    current: StrategyId,
    /// Whether its status says it is stalled.
    stalled: bool,
}

/// `millis` of time and `vram` bytes.
fn option(id: StrategyId, millis: u64, vram: u64) -> StrategyOption {
    StrategyOption {
        id,
        estimated_time: Duration::from_millis(millis),
        estimated_vram: vram,
    }
}

impl Old {
    /// Offers LowPower at 2 ms, Balanced at 4 ms and 1000 bytes, and
    /// HighPerformance at 8 ms and 2000 bytes — listed dearest first, so a
    /// bridge that takes the list's order for the cost order is caught.
    fn new() -> Self {
        Self {
            options: vec![
                option(StrategyId::HighPerformance, 8, 2000),
                option(StrategyId::LowPower, 2, 0),
                option(StrategyId::Balanced, 4, 1000),
            ],
            applied: Vec::new(),
            asked: Vec::new(),
            current: StrategyId::Balanced,
            stalled: false,
        }
    }

    /// The strategy of the last budget applied.
    fn last_applied(&self) -> Option<StrategyId> {
        self.applied.last().map(|budget| budget.strategy_id)
    }
}

impl Agent for Old {
    fn id(&self) -> AgentId {
        AgentId::new("script")
    }

    fn negotiate(&mut self, request: NegotiationRequest) -> NegotiationResponse {
        self.asked.push(request);
        NegotiationResponse {
            strategies: self.options.clone(),
            timing_adjustment: None,
        }
    }

    fn apply_budget(&mut self, budget: ResourceBudget) {
        self.current = budget.strategy_id;
        self.applied.push(budget);
    }

    fn report_status(&self) -> AgentStatus {
        AgentStatus {
            agent_id: AgentId::new("script"),
            current_strategy: self.current,
            health_score: 0.75,
            is_stalled: self.stalled,
            message: String::new(),
        }
    }

    fn execute(&mut self, _context: &mut EngineContext<'_>) {}

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// A request for epoch 3, at a 60 Hz target, while playing.
fn request() -> OfferRequest {
    OfferRequest {
        epoch: Epoch(3),
        target: Duration::from_micros(16_667),
        pressure: 0.0,
        mode: EngineMode::Playing,
        reply_by: Instant::now() + Duration::from_secs(1),
    }
}

/// An envelope of `millis` on the host and `memory` bytes on every device.
fn envelope(millis: u64, memory: u64) -> Envelope {
    Envelope {
        time: PerDevice {
            cpu: Duration::from_millis(millis),
            ..PerDevice::default()
        },
        memory: PerDevice {
            cpu: memory,
            gpu: memory,
            npu: memory,
            other: memory,
        },
    }
}

/// The time a cost takes: the busiest device's.
fn time_of(cost: &Cost) -> Duration {
    cost.time
        .cpu
        .max(cost.time.gpu)
        .max(cost.time.npu)
        .max(cost.time.other)
}

/// **An old agent offers and applies through the bridge.** It implements only
/// `negotiate`, `apply_budget` and `report_status`. Asked for an offer, it
/// offers its three options as rows, names the cheapest as its floor and the
/// running one as current; lent an envelope of 5 ms, it is applied the
/// dearest option that fits, Balanced at 4 ms.
#[test]
fn old_agents_answer_through_the_bridge() {
    let mut agent = Old::new();

    let offer = agent.offer(&request());

    assert_eq!(offer.agent, AgentId::new("script"));
    assert_eq!(
        offer.epoch,
        Epoch(3),
        "the offer is for the request's epoch"
    );
    let rows: Vec<(StrategyId, Duration, u64)> = offer
        .rows
        .iter()
        .map(|row| (row.id, time_of(&row.cost), row.cost.memory.gpu))
        .collect();
    assert_eq!(
        rows,
        vec![
            (StrategyId::HighPerformance, Duration::from_millis(8), 2000),
            (StrategyId::LowPower, Duration::from_millis(2), 0),
            (StrategyId::Balanced, Duration::from_millis(4), 1000),
        ],
        "every option is a row, at its time and its graphics memory"
    );
    assert_eq!(
        offer.floor,
        StrategyId::LowPower,
        "the floor is the cheapest"
    );
    assert_eq!(offer.current, StrategyId::Balanced, "the running option");
    assert_eq!(offer.health, 0.75, "the status's health");

    agent.apply(Epoch(3), &envelope(5, u64::MAX), None, None);

    assert_eq!(
        agent.last_applied(),
        Some(StrategyId::Balanced),
        "the dearest option within 5 ms is Balanced"
    );
}

/// The bridge asks `negotiate` with the request's target and mode, so an old
/// agent sees the frame it is offering for.
#[test]
fn the_bridge_offer_asks_negotiate_for_the_requests_target_and_mode() {
    let mut agent = Old::new();

    let _ = agent.offer(&request());

    let asked = agent.asked.last().expect("the bridge asked negotiate");
    assert_eq!(asked.target_latency, Duration::from_micros(16_667));
    assert_eq!(asked.current_mode, EngineMode::Playing);
}

/// **The bridge applies the dearest option that fits on time and memory**,
/// with the envelope's time as the budget's limit. Within 10 ms every option
/// fits on time, but HighPerformance holds 2000 bytes and the envelope lends
/// 1500: Balanced is applied, limited to 10 ms.
#[test]
fn the_bridge_apply_picks_the_dearest_option_that_fits_the_envelope() {
    let mut agent = Old::new();

    agent.apply(Epoch(1), &envelope(10, 1500), None, None);

    let applied = agent.applied.last().expect("a budget was applied");
    assert_eq!(
        applied.strategy_id,
        StrategyId::Balanced,
        "HighPerformance holds more memory than the envelope lends"
    );
    assert_eq!(
        applied.time_limit,
        Duration::from_millis(10),
        "the budget's limit is the envelope's time"
    );
}

/// An envelope no option fits applies the cheapest, as the floor would be.
#[test]
fn the_bridge_apply_falls_back_to_the_cheapest_option_when_none_fits() {
    let mut agent = Old::new();

    agent.apply(Epoch(1), &envelope(1, u64::MAX), None, None);

    assert_eq!(
        agent.last_applied(),
        Some(StrategyId::LowPower),
        "nothing fits 1 ms: the cheapest option"
    );
}

/// A pin is applied whatever fits, and an allowed list filters the options
/// the bridge picks from.
#[test]
fn the_bridge_apply_follows_a_pin_and_the_allowed_rows() {
    let mut pinned = Old::new();
    pinned.apply(
        Epoch(1),
        &envelope(1, u64::MAX),
        Some(StrategyId::HighPerformance),
        None,
    );
    assert_eq!(
        pinned.last_applied(),
        Some(StrategyId::HighPerformance),
        "a pin overrides the fit"
    );

    let mut bounded = Old::new();
    bounded.apply(
        Epoch(1),
        &envelope(10, u64::MAX),
        None,
        Some(&[StrategyId::LowPower, StrategyId::Balanced]),
    );
    assert_eq!(
        bounded.last_applied(),
        Some(StrategyId::Balanced),
        "HighPerformance fits but is not allowed"
    );
}

/// **The bridge reports from `report_status`**: the agent, the row it runs
/// and its health.
#[test]
fn the_bridge_report_reads_report_status() {
    let mut agent = Old::new();
    agent.apply_budget(ResourceBudget {
        strategy_id: StrategyId::HighPerformance,
        time_limit: Duration::from_millis(8),
        memory_limit: None,
        extra_params: Default::default(),
    });

    let report = agent.report();

    assert_eq!(report.agent, AgentId::new("script"));
    assert_eq!(report.row, StrategyId::HighPerformance);
    assert_eq!(report.health, 0.75);
}

/// **The bridge fits an option on the device it offered it on.** The bridge
/// offers each option's time on the host. Lent 3 ms on the host and 10 ms on
/// the graphics device, only LowPower, 2 ms, fits the host: HighPerformance,
/// offered at 8 ms on the host, does not fit there, though it fits the
/// envelope's busiest device.
#[test]
fn the_bridge_apply_fits_each_option_on_the_host_it_was_offered_on() {
    let mut agent = Old::new();
    let lent = Envelope {
        time: PerDevice {
            cpu: Duration::from_millis(3),
            gpu: Duration::from_millis(10),
            ..PerDevice::default()
        },
        memory: PerDevice {
            cpu: u64::MAX,
            gpu: u64::MAX,
            npu: u64::MAX,
            other: u64::MAX,
        },
    };

    agent.apply(Epoch(1), &lent, None, None);

    assert_eq!(
        agent.last_applied(),
        Some(StrategyId::LowPower),
        "the only option within the host's 3 ms"
    );
}

// ─── An agent id is its name ────────────────────────────────────────────────

/// **An agent id is its name.** `AgentId::new("ui")` equals itself and no
/// other name, displays and serialises as `ui`, and reads back from that.
#[test]
fn an_agent_id_is_its_name() {
    let ui = AgentId::new("ui");

    assert_eq!(ui, AgentId::new("ui"), "one name, one agent");
    assert_ne!(ui, AgentId::new("audio"), "two names, two agents");
    assert_eq!(ui.name(), "ui");
    assert_eq!(ui.to_string(), "ui", "Display prints the name");

    let written = serde_json::to_string(&ui).expect("an agent id serialises");
    assert_eq!(written, "\"ui\"", "an agent id is written as its name");
    let read: AgentId = serde_json::from_str(&written).expect("an agent id reads back");
    assert_eq!(read, ui, "an agent id round-trips");
    assert_eq!(read.name(), "ui");
}

/// **Agent ids order by name**, not by a declaration order: a journal keyed
/// by agent lists them the same way on every run. `physics` comes before
/// `renderer`, which the old variant order put first.
#[test]
fn agent_ids_order_by_name() {
    let mut ids = [
        AgentId::new("ui"),
        AgentId::new("renderer"),
        AgentId::new("audio"),
        AgentId::new("physics"),
    ];

    ids.sort();

    assert!(AgentId::new("physics") < AgentId::new("renderer"));
    assert!(AgentId::new("audio") < AgentId::new("ui"));
    let names: Vec<&str> = ids.iter().map(|id| id.name()).collect();
    assert_eq!(names, ["audio", "physics", "renderer", "ui"]);

    let journal: std::collections::BTreeMap<AgentId, u32> =
        [(AgentId::new("ui"), 1), (AgentId::new("audio"), 2)].into();
    let keys: Vec<&str> = journal.keys().map(|id| id.name()).collect();
    assert_eq!(
        keys,
        ["audio", "ui"],
        "a journal keyed by agent lists names in order"
    );
}

/// **The bridge report says when the agent is stalled**: an old agent whose
/// status says it is stalled reports it, so the DCC learns of the stall from
/// the report alone, without asking the agent.
#[test]
fn the_bridge_report_says_when_the_agent_is_stalled() {
    let mut agent = Old::new();
    assert!(!agent.report().stalled, "a running agent is not stalled");

    agent.stalled = true;

    assert!(agent.report().stalled, "its status says it is stalled");
}

// ─── The journal ────────────────────────────────────────────────────────────

/// Two agents whose names appear nowhere else in a journal.
const ALPHA: AgentId = AgentId::new("journal-alpha");
const BETA: AgentId = AgentId::new("journal-beta");

/// `micros` on the host, nothing elsewhere.
fn host_time(micros: u64) -> PerDevice<Duration> {
    PerDevice {
        cpu: Duration::from_micros(micros),
        ..PerDevice::default()
    }
}

/// Epoch `epoch`, naming both agents in every place a record names an agent.
fn epoch_record(epoch: u64) -> EpochRecord {
    let envelope = |micros| Envelope {
        time: host_time(micros),
        memory: PerDevice {
            gpu: 1024,
            ..PerDevice::default()
        },
    };
    let offer = |agent, confidence| Offer {
        agent,
        epoch: Epoch(epoch),
        rows: vec![
            OfferRow {
                id: StrategyId::Balanced,
                cost: Cost {
                    time: host_time(2_000),
                    ..Cost::default()
                },
                quality: 0.7,
                confidence,
            },
            OfferRow {
                id: StrategyId::Custom(3),
                cost: Cost {
                    time: host_time(500),
                    ..Cost::default()
                },
                quality: 0.4,
                confidence: Confidence::Assumed,
            },
        ],
        floor: StrategyId::Custom(3),
        current: StrategyId::Balanced,
        health: 0.9,
    };
    let report = |agent| Report {
        agent,
        epoch: Epoch(epoch - 1),
        row: StrategyId::Balanced,
        measured: host_time(1_500),
        done: 1.0,
        health: 0.9,
        over_budget: epoch.is_multiple_of(2),
        stalled: false,
        workload: Some(120.0),
    };
    EpochRecord {
        allocation: Allocation {
            epoch: Epoch(epoch),
            trigger: if epoch == 1 {
                Trigger::Initial
            } else {
                Trigger::Overruns
            },
            envelopes: vec![(ALPHA, envelope(4_000)), (BETA, envelope(3_000))],
            pins: vec![(ALPHA, StrategyId::LowPower)],
            allowed: vec![(BETA, vec![StrategyId::LowPower, StrategyId::Balanced])],
            price: Some(0.25),
            late: vec![BETA],
        },
        target: Duration::from_micros(16_667),
        offers: vec![
            offer(ALPHA, Confidence::Measured),
            offer(BETA, Confidence::Quoted),
        ],
        reports: vec![report(ALPHA), report(BETA)],
        waves: vec![vec![ALPHA, BETA]],
        policies: vec![
            (ALPHA, AdaptationMode::Manual(StrategyId::LowPower)),
            (
                BETA,
                AdaptationMode::Bounded {
                    min: StrategyId::LowPower,
                    max: StrategyId::Balanced,
                },
            ),
        ],
        hints: vec![(
            BETA,
            AgentHints {
                cap_ms: Some(8.0),
                priority: Some(0.5),
            },
        )],
        reasons: vec!["the frame ran over its target".to_owned()],
    }
}

/// A journal of `epochs` epochs, each naming both agents.
fn journal(epochs: u64) -> DecisionTrace {
    DecisionTrace {
        epochs: (1..=epochs).map(epoch_record).collect(),
    }
}

/// Every agent `trace` names, wherever it names one.
fn every_agent_named(trace: &DecisionTrace) -> Vec<AgentId> {
    let mut named = Vec::new();
    for record in &trace.epochs {
        let allocation = &record.allocation;
        named.extend(allocation.envelopes.iter().map(|(agent, _)| *agent));
        named.extend(allocation.pins.iter().map(|(agent, _)| *agent));
        named.extend(allocation.allowed.iter().map(|(agent, _)| *agent));
        named.extend(allocation.late.iter().copied());
        named.extend(record.offers.iter().map(|offer| offer.agent));
        named.extend(record.reports.iter().map(|report| report.agent));
        named.extend(record.waves.iter().flatten().copied());
        named.extend(record.policies.iter().map(|(agent, _)| *agent));
        named.extend(record.hints.iter().map(|(agent, _)| *agent));
    }
    named
}

/// **A journal round-trips through serde**: every epoch's allocation, target,
/// offers, reports, waves, policies, hints and reasons read back as written.
#[test]
fn a_journal_round_trips_through_serde() {
    let trace = journal(3);

    let written = serde_json::to_string(&trace).expect("a journal serialises");
    let read: DecisionTrace = serde_json::from_str(&written).expect("a journal reads back");

    assert_eq!(read, trace);
}

/// **A journal writes each agent's name once**, in a table its entries refer
/// to: two hundred epochs, each naming the two agents fourteen times between
/// them, still hold each name a single time.
#[test]
fn a_journal_writes_each_agent_name_once() {
    let written = serde_json::to_string(&journal(200)).expect("a journal serialises");

    assert_eq!(written.matches(ALPHA.name()).count(), 1, "{ALPHA} once");
    assert_eq!(written.matches(BETA.name()).count(), 1, "{BETA} once");
}

/// **Reading a journal keeps one name per agent.** An agent id read back keeps
/// its name for the program's life; a journal of two hundred epochs naming two
/// agents reads back with every mention of an agent sharing that agent's one
/// name — two names kept, not one per mention.
#[test]
fn reading_a_journal_keeps_one_name_per_agent() {
    let written = serde_json::to_string(&journal(200)).expect("a journal serialises");
    let read: DecisionTrace = serde_json::from_str(&written).expect("a journal reads back");

    let named = every_agent_named(&read);
    assert_eq!(named.len(), 200 * 14, "every mention read back");
    let kept: std::collections::HashSet<usize> = named
        .iter()
        .map(|agent| agent.name().as_ptr() as usize)
        .collect();
    assert_eq!(
        kept.len(),
        2,
        "one name kept per agent, whatever the number of mentions"
    );
    let names: std::collections::BTreeSet<&str> = named.iter().map(|agent| agent.name()).collect();
    assert_eq!(names, [ALPHA.name(), BETA.name()].into());
}

/// **A journal keeps a free upgrade's price.** An upgrade that costs no more
/// time than the row below it — two rows quoting the same time, as every row
/// of an idle family quotes zero — gains an infinite weighed quality per
/// microsecond, and `decide` records that as the round's price. Written and
/// read back, the epoch still says the price was infinite, not that the round
/// took no upgrade.
#[test]
fn a_journal_keeps_a_free_upgrades_price() {
    let mut trace = journal(1);
    trace.epochs[0].allocation.price = Some(f32::INFINITY);

    let written = serde_json::to_string(&trace).expect("a journal serialises");
    let read: DecisionTrace = serde_json::from_str(&written).expect("a journal reads back");

    assert_eq!(
        read.epochs[0].allocation.price,
        Some(f32::INFINITY),
        "written as {written}"
    );
}

/// **A journal naming an agent its table does not hold is refused**, with the
/// index and the table's size: an entry is never read back as some other
/// agent, nor dropped.
#[test]
fn a_journal_naming_an_agent_outside_its_table_is_refused() {
    let written = serde_json::to_string(&journal(1)).expect("a journal serialises");
    let mut value: serde_json::Value = serde_json::from_str(&written).expect("a journal is JSON");
    value["epochs"][0]["allocation"]["late"] = serde_json::json!([7]);

    let read = serde_json::from_value::<DecisionTrace>(value);

    let error = read
        .expect_err("agent #7 is not in a table of two")
        .to_string();
    assert!(
        error.contains("#7") && error.contains('2'),
        "the index and the table's size: {error}"
    );
}

/// **A journal whose price is not a number still reads back.** A priority
/// hint of NaN (`EngineHint::Prioritize`, which nothing checks) weighs every
/// gain NaN; `total_cmp` orders a NaN slope above all others, so the round
/// takes that upgrade and records a NaN price. The journal written then reads
/// back, its price still not a number — one bad hint does not make a whole
/// session's journal unreadable.
#[test]
fn a_journal_whose_price_is_not_a_number_still_reads_back() {
    let mut trace = journal(2);
    trace.epochs[1].allocation.price = Some(f32::NAN);

    let written = serde_json::to_string(&trace).expect("a journal serialises");
    let read = serde_json::from_str::<DecisionTrace>(&written);

    let read = read.unwrap_or_else(|error| panic!("{error}: written as {written}"));
    assert!(
        read.epochs[1].allocation.price.is_some_and(f32::is_nan),
        "{:?}",
        read.epochs[1].allocation.price
    );
}
