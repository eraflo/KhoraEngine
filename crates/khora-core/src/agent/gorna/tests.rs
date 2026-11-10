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

//! GORNA's types: an agent's id, the journal of decisions, and the frame
//! report.

use std::time::Duration;

use super::protocol::{
    Allocation, Binding, Confidence, DecidedBy, Envelope, Epoch, FrameReport, Offer, OfferRow,
    Rejected, Report, Trigger, Why,
};
use super::{
    AdaptationMode, AgentHints, AgentId, Correction, DecisionTrace, EpochRecord, FrameSummary,
    Planning, StrategyId,
};
use crate::agent::AgentImportance;
use crate::lane::cost::{Cost, Device, PerDevice};

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
        switch: None,
        message: None,
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
            lever: false,
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
        why: Why::default(),
        importance: Vec::new(),
        corrections: Vec::new(),
        memory: PerDevice::default(),
        background: Vec::new(),
        at: Duration::ZERO,
        frames: FrameSummary::default(),
        in_force: Vec::new(),
        planning: Planning::default(),
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

/// **A journal keeps whether an epoch left a lever**, and one written before
/// the lever was recorded reads back as leaving none. A replay puts the
/// recorded allocation in force, and the frame governor reads its lever: an
/// epoch that left one must not read back as all floors, nor the reverse.
#[test]
fn a_journal_keeps_whether_an_epoch_left_a_lever() {
    let mut trace = journal(2);
    trace.epochs[0].allocation.lever = true;

    let written = serde_json::to_string(&trace).expect("a journal serialises");
    let read: DecisionTrace = serde_json::from_str(&written).expect("a journal reads back");
    assert!(read.epochs[0].allocation.lever, "written as {written}");
    assert!(!read.epochs[1].allocation.lever, "written as {written}");

    let mut older: serde_json::Value = serde_json::from_str(&written).expect("a journal is JSON");
    for epoch in older["epochs"].as_array_mut().expect("the epochs") {
        epoch["allocation"]
            .as_object_mut()
            .expect("an allocation")
            .remove("lever")
            .expect("the lever is written");
    }
    let read = serde_json::from_value::<DecisionTrace>(older).expect("an older journal reads");
    assert!(
        read.epochs.iter().all(|epoch| !epoch.allocation.lever),
        "a journal without levers leaves none"
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

/// A round decided by the Q-RAM pass, bound by the graphics processor's
/// time, that refused one upgrade of each agent: `ALPHA`'s for the graphics
/// processor's time, `BETA`'s for its memory.
fn a_greedy_why() -> Why {
    Why {
        by: DecidedBy::Greedy,
        trigger: Trigger::Overruns,
        binding: Some(Binding::Time(Device::Gpu)),
        rejected: vec![
            Rejected {
                agent: ALPHA,
                row: StrategyId::HighPerformance,
                over: Binding::Time(Device::Gpu),
            },
            Rejected {
                agent: BETA,
                row: StrategyId::Custom(9),
                over: Binding::Memory(Device::Gpu),
            },
        ],
    }
}

/// **A journal keeps why each epoch was decided, and what a switch of row
/// cost**, and one written before it kept them reads back decided by the
/// floors, nothing binding, nothing rejected, no switch measured. A replay
/// or an evaluation reads the reasons and the switch costs back from the
/// journal: an epoch that recorded them must not read back without them.
#[test]
fn the_journal_keeps_the_why_and_the_switch() {
    let mut trace = journal(2);
    trace.epochs[0].why = a_greedy_why();
    trace.epochs[0].reports[0].switch = Some(Duration::from_micros(750));

    let written = serde_json::to_string(&trace).expect("a journal serialises");
    let read: DecisionTrace = serde_json::from_str(&written).expect("a journal reads back");
    assert_eq!(read.epochs[0].why, a_greedy_why(), "written as {written}");
    assert_eq!(
        read.epochs[0].reports[0].switch,
        Some(Duration::from_micros(750)),
        "written as {written}"
    );
    assert_eq!(read, trace, "every epoch reads back as written");

    let mut older: serde_json::Value = serde_json::from_str(&written).expect("a journal is JSON");
    for epoch in older["epochs"].as_array_mut().expect("the epochs") {
        epoch
            .as_object_mut()
            .expect("an epoch")
            .remove("why")
            .expect("the why is written");
        for report in epoch["reports"].as_array_mut().expect("the reports") {
            report
                .as_object_mut()
                .expect("a report")
                .remove("switch")
                .expect("the switch is written");
        }
    }
    let read = serde_json::from_value::<DecisionTrace>(older).expect("an older journal reads");
    for epoch in &read.epochs {
        assert_eq!(epoch.why.by, DecidedBy::Floors, "{:?}", epoch.why);
        assert_eq!(epoch.why.binding, None, "{:?}", epoch.why);
        assert!(epoch.why.rejected.is_empty(), "{:?}", epoch.why);
        assert!(
            epoch.reports.iter().all(|report| report.switch.is_none()),
            "no switch measured: {:?}",
            epoch.reports
        );
    }
}

/// **A journal names a refused upgrade's agent through its table.** Two
/// hundred epochs, each refusing an upgrade of both agents, read back as
/// written and still hold each name a single time: the reasons cost no name
/// per epoch, written or read.
#[test]
fn a_journal_names_a_refused_upgrades_agent_once() {
    let mut trace = journal(200);
    for epoch in &mut trace.epochs {
        epoch.why = a_greedy_why();
    }

    let written = serde_json::to_string(&trace).expect("a journal serialises");
    let read: DecisionTrace = serde_json::from_str(&written).expect("a journal reads back");

    assert_eq!(read, trace, "the reasons read back as written");
    assert_eq!(written.matches(ALPHA.name()).count(), 1, "{ALPHA} once");
    assert_eq!(written.matches(BETA.name()).count(), 1, "{BETA} once");
}

/// **A journal written before the reasons reads each epoch's own trigger.**
/// Its epochs were opened by `Initial`, then by `Overruns`: an epoch read
/// back without a written `why` says it was opened by what its allocation
/// says, not by `Initial`.
#[test]
fn an_older_journal_reads_each_epochs_own_trigger() {
    let written = serde_json::to_string(&journal(3)).expect("a journal serialises");
    let mut older: serde_json::Value = serde_json::from_str(&written).expect("a journal is JSON");
    for epoch in older["epochs"].as_array_mut().expect("the epochs") {
        epoch
            .as_object_mut()
            .expect("an epoch")
            .remove("why")
            .expect("the why is written");
    }

    let read = serde_json::from_value::<DecisionTrace>(older).expect("an older journal reads");

    for epoch in &read.epochs {
        assert_eq!(
            epoch.why.trigger, epoch.allocation.trigger,
            "epoch {:?}: {:?}",
            epoch.allocation.epoch, epoch.why
        );
    }
}

/// `record` holding what its round read besides the offers and the
/// reports: both agents' importance, a graphics memory limit, `BETA` at the background, when the epoch opened, how the
/// frames ran since the one before, and the envelopes both agents held when
/// the round opened.
fn with_the_rounds_inputs(mut record: EpochRecord) -> EpochRecord {
    let epoch = record.allocation.epoch.0;
    record.importance = vec![
        (ALPHA, AgentImportance::Critical),
        (BETA, AgentImportance::Optional),
    ];
    record.memory = PerDevice {
        gpu: Some(512 * 1024 * 1024),
        ..PerDevice::default()
    };
    record.background = vec![BETA];
    record.at = Duration::from_millis(1_250 * epoch);
    record.frames = FrameSummary {
        count: 30 + epoch as u32,
        over_target: 2,
        worst: Duration::from_micros(41_500),
        mean: Duration::from_micros(15_250),
    };
    record.in_force = vec![
        (
            ALPHA,
            Envelope {
                time: host_time(3_000 + epoch),
                ..Envelope::default()
            },
        ),
        (
            BETA,
            Envelope {
                time: host_time(1_500),
                memory: PerDevice {
                    gpu: 2048,
                    ..PerDevice::default()
                },
            },
        ),
    ];
    record
}

/// A journal of `epochs` epochs, each naming both agents and holding what
/// its round read.
fn journal_with_the_rounds_inputs(epochs: u64) -> DecisionTrace {
    DecisionTrace {
        epochs: (1..=epochs)
            .map(|epoch| with_the_rounds_inputs(epoch_record(epoch)))
            .collect(),
    }
}

/// **The journal keeps what each round read, and how the frames ran.** Each
/// agent's importance, the memory limits, the agents at the background, when the epoch opened, the
/// frames since the one before and the envelopes in force when the round
/// opened read back as written: with them, an epoch can be decided again as
/// it was, even one recorded after the DCC had been running. Two hundred epochs naming both
/// agents in these lists too still write each name a single time.
#[test]
fn the_journal_keeps_the_rounds_inputs_and_the_frames() {
    let trace = journal_with_the_rounds_inputs(3);

    let written = serde_json::to_string(&trace).expect("a journal serialises");
    let read: DecisionTrace = serde_json::from_str(&written).expect("a journal reads back");

    let first = &read.epochs[0];
    assert_eq!(
        first.importance,
        vec![
            (ALPHA, AgentImportance::Critical),
            (BETA, AgentImportance::Optional)
        ],
        "written as {written}"
    );
    assert_eq!(
        first.memory.gpu,
        Some(512 * 1024 * 1024),
        "written as {written}"
    );
    assert_eq!(first.memory.cpu, None, "written as {written}");
    assert_eq!(first.background, vec![BETA], "written as {written}");
    assert_eq!(
        first.at,
        Duration::from_millis(1_250),
        "written as {written}"
    );
    assert_eq!(
        first.frames,
        FrameSummary {
            count: 31,
            over_target: 2,
            worst: Duration::from_micros(41_500),
            mean: Duration::from_micros(15_250),
        },
        "written as {written}"
    );
    assert_eq!(
        first.in_force,
        vec![
            (
                ALPHA,
                Envelope {
                    time: host_time(3_001),
                    ..Envelope::default()
                },
            ),
            (
                BETA,
                Envelope {
                    time: host_time(1_500),
                    memory: PerDevice {
                        gpu: 2048,
                        ..PerDevice::default()
                    },
                },
            ),
        ],
        "written as {written}"
    );
    assert_eq!(read, trace, "every epoch reads back as written");

    let long =
        serde_json::to_string(&journal_with_the_rounds_inputs(200)).expect("a journal serialises");
    assert_eq!(long.matches(ALPHA.name()).count(), 1, "{ALPHA} once");
    assert_eq!(long.matches(BETA.name()).count(), 1, "{BETA} once");
}

/// **A journal written before the rounds' inputs were kept reads without
/// them.** Its epochs carry no importance, memory limits,
/// background, time, frames nor envelopes in force: they read back empty,
/// unknown and zero, and everything else as written.
#[test]
fn an_older_journal_reads_without_the_rounds_inputs() {
    let trace = journal_with_the_rounds_inputs(2);
    let written = serde_json::to_string(&trace).expect("a journal serialises");
    let mut older: serde_json::Value = serde_json::from_str(&written).expect("a journal is JSON");
    for epoch in older["epochs"].as_array_mut().expect("the epochs") {
        let epoch = epoch.as_object_mut().expect("an epoch");
        for field in [
            "importance",
            "memory",
            "background",
            "at",
            "frames",
            "in_force",
        ] {
            epoch
                .remove(field)
                .unwrap_or_else(|| panic!("the {field} is written: {written}"));
        }
    }

    let read = serde_json::from_value::<DecisionTrace>(older).expect("an older journal reads");

    assert_eq!(read.epochs.len(), 2);
    for (read, written) in read.epochs.iter().zip(&trace.epochs) {
        assert!(read.importance.is_empty(), "{:?}", read.importance);
        assert_eq!(read.memory, PerDevice::default(), "no limit known");
        assert!(read.background.is_empty(), "{:?}", read.background);
        assert_eq!(read.at, Duration::ZERO);
        assert_eq!(read.frames, FrameSummary::default());
        assert!(read.in_force.is_empty(), "{:?}", read.in_force);
        assert_eq!(read.allocation, written.allocation);
        assert_eq!(read.offers, written.offers);
        assert_eq!(read.reports, written.reports);
        assert_eq!(read.why, written.why);
    }
}

// ── What a round planned for ──────────────────────────────────────

/// A planning naming both agents: a 5 s horizon, a 250 ms transient, a
/// 1.5 ms margin, `ALPHA` growing by 1.25 and `BETA` by 0.8, a switch of
/// `ALPHA` to `Balanced` costing 3 ms and of `BETA` to `Custom(3)` 750 µs,
/// and each row planned the 5 % risk's z — about 1.645 — spreads above its
/// correction.
fn a_planning() -> Planning {
    Planning {
        horizon: Duration::from_secs(5),
        transient: Duration::from_millis(250),
        margin: Duration::from_micros(1_500),
        growth: vec![(ALPHA, 1.25), (BETA, 0.8)],
        switch: vec![
            (ALPHA, StrategyId::Balanced, Duration::from_millis(3)),
            (BETA, StrategyId::Custom(3), Duration::from_micros(750)),
        ],
        caution: 1.644_853_626_951_472_2,
    }
}

/// **The default planning plans for nothing beyond the frame.** No horizon,
/// transient, margin nor caution; an agent it does not name grows by 1, and
/// a row it does not name switches free.
#[test]
fn the_default_planning_plans_for_nothing_beyond_the_frame() {
    let planning = Planning::default();

    assert_eq!(planning.horizon, Duration::ZERO);
    assert_eq!(planning.transient, Duration::ZERO);
    assert_eq!(planning.margin, Duration::ZERO);
    assert_eq!(planning.caution, 0.0, "each row planned at its factor");
    assert_eq!(
        planning.growth_of(ALPHA),
        1.0,
        "an agent not named grows by 1"
    );
    assert_eq!(
        planning.switch_of(ALPHA, StrategyId::Balanced),
        None,
        "a row not named switches free"
    );
}

/// **A planning reads each agent's growth and each row's switch.** `ALPHA`
/// grows by 1.25, `BETA` by 0.8, an agent it does not name by 1; the switch
/// of `ALPHA` to `Balanced` costs 3 ms, of `BETA` to `Custom(3)` 750 µs, and
/// a row it names for one agent says nothing of another's.
#[test]
fn a_planning_reads_each_agents_growth_and_each_rows_switch() {
    let planning = a_planning();
    let other = AgentId::new("journal-gamma");

    assert_eq!(planning.growth_of(ALPHA), 1.25);
    assert_eq!(planning.growth_of(BETA), 0.8);
    assert_eq!(planning.growth_of(other), 1.0);
    assert_eq!(
        planning.switch_of(ALPHA, StrategyId::Balanced),
        Some(Duration::from_millis(3))
    );
    assert_eq!(
        planning.switch_of(BETA, StrategyId::Custom(3)),
        Some(Duration::from_micros(750))
    );
    assert_eq!(
        planning.switch_of(ALPHA, StrategyId::Custom(3)),
        None,
        "BETA's row says nothing of ALPHA's"
    );
    assert_eq!(planning.switch_of(BETA, StrategyId::Balanced), None);
    assert_eq!(planning.switch_of(other, StrategyId::Balanced), None);
}

/// **The journal keeps each round's planning**: its horizon, transient,
/// margin and caution, each agent's growth and each row's switch read back
/// as written,
/// on the epoch that planned them, and the other epoch reads back planning
/// nothing. Two hundred epochs naming both agents in their planning too
/// still write each name a single time.
#[test]
fn the_journal_keeps_each_rounds_planning() {
    let mut trace = journal(2);
    trace.epochs[0].planning = a_planning();

    let written = serde_json::to_string(&trace).expect("a journal serialises");
    let read: DecisionTrace = serde_json::from_str(&written).expect("a journal reads back");

    assert_eq!(
        read.epochs[0].planning,
        a_planning(),
        "written as {written}"
    );
    assert_eq!(
        read.epochs[1].planning,
        Planning::default(),
        "written as {written}"
    );
    assert_eq!(read, trace, "every epoch reads back as written");

    let mut long = journal(200);
    for epoch in &mut long.epochs {
        epoch.planning = a_planning();
    }
    let long = serde_json::to_string(&long).expect("a journal serialises");
    assert_eq!(long.matches(ALPHA.name()).count(), 1, "{ALPHA} once");
    assert_eq!(long.matches(BETA.name()).count(), 1, "{BETA} once");
}

/// **The journal keeps each growth to the bit.** Replay decides a round again
/// from the growth it reads back, so a growth computed by the planner — any
/// `f64`, not a short decimal — reads back exactly as it was planned.
#[test]
fn the_journal_keeps_each_growth_to_the_bit() {
    let mut trace = journal(1);
    trace.epochs[0].planning.growth = (1..=2_000_u32)
        .map(|k| {
            let growth = (170.0 + f64::from(k) / 3.0) / (120.0 + f64::from(k).sqrt());
            (AgentId::new("journal-alpha"), growth)
        })
        .collect();

    let written = serde_json::to_string(&trace).expect("a journal serialises");
    let read: DecisionTrace = serde_json::from_str(&written).expect("a journal reads back");

    for ((_, read), (_, planned)) in
        (read.epochs[0].planning.growth.iter()).zip(&trace.epochs[0].planning.growth)
    {
        assert_eq!(
            read.to_bits(),
            planned.to_bits(),
            "planned {planned:?}, read back {read:?}"
        );
    }
}

/// **A journal written before the planning was kept reads no planning.** Its
/// epochs carry no `planning`: they read back planning nothing beyond the
/// frame, and everything else as written.
#[test]
fn an_older_journal_reads_no_planning() {
    let mut trace = journal(2);
    for epoch in &mut trace.epochs {
        epoch.planning = a_planning();
    }
    let written = serde_json::to_string(&trace).expect("a journal serialises");
    let mut older: serde_json::Value = serde_json::from_str(&written).expect("a journal is JSON");
    for epoch in older["epochs"].as_array_mut().expect("the epochs") {
        epoch
            .as_object_mut()
            .expect("an epoch")
            .remove("planning")
            .expect("the planning is written");
    }

    let read = serde_json::from_value::<DecisionTrace>(older).expect("an older journal reads");

    assert_eq!(read.epochs.len(), 2);
    for (read, written) in read.epochs.iter().zip(&trace.epochs) {
        assert_eq!(read.planning, Planning::default());
        assert_eq!(read.allocation, written.allocation);
        assert_eq!(read.offers, written.offers);
        assert_eq!(read.reports, written.reports);
        assert_eq!(read.why, written.why);
    }
}

// ── What each row's host time was corrected by ────────────────────

/// **A correction is planned `caution` spreads above its factor.** A row
/// learned at 2 with a spread of 0.5 is planned at 2 + 1.645 × 0.5 under
/// the 5 % risk's caution, and at its factor under none; the default
/// correction — factor 1, no spread — plans every row at its quote,
/// whatever the caution.
#[test]
fn a_correction_is_planned_caution_spreads_above_its_factor() {
    let learned = Correction {
        factor: 2.0,
        spread: 0.5,
    };

    assert!(
        (learned.planned(1.645) - 2.8225).abs() < 1e-12,
        "2 + 1.645 × 0.5: {}",
        learned.planned(1.645)
    );
    assert_eq!(learned.planned(0.0), 2.0, "no caution, the factor");
    assert_eq!(
        Correction::default(),
        Correction {
            factor: 1.0,
            spread: 0.0
        }
    );
    assert_eq!(Correction::default().planned(1.645), 1.0);
    assert_eq!(Correction::default().planned(3.0), 1.0);
}

/// The corrections a round of `ALPHA` and `BETA` read: `ALPHA`'s `Balanced`
/// learned a little above 4/3 (a factor that is no short decimal), its
/// `Custom(3)` never run, `BETA`'s `Balanced` at 2 with a wide spread.
fn some_corrections() -> Vec<(AgentId, StrategyId, Correction)> {
    vec![
        (
            ALPHA,
            StrategyId::Balanced,
            Correction {
                factor: 4.0 / 3.0 + 1e-9,
                spread: 0.1_f64.sqrt(),
            },
        ),
        (
            ALPHA,
            StrategyId::Custom(3),
            Correction {
                factor: 1.0,
                spread: 0.5,
            },
        ),
        (
            BETA,
            StrategyId::Balanced,
            Correction {
                factor: 2.0,
                spread: 0.75,
            },
        ),
    ]
}

/// **The journal keeps each row's correction.** The epoch whose round read
/// them reads back every (agent, row, factor, spread) as written, each
/// factor and spread to the bit — a replay decides again on them — and the
/// other epoch reads back none. Two hundred epochs naming both agents in
/// their corrections still write each name a single time.
#[test]
fn the_journal_keeps_each_rows_correction() {
    let mut trace = journal(2);
    trace.epochs[0].corrections = some_corrections();

    let written = serde_json::to_string(&trace).expect("a journal serialises");
    let read: DecisionTrace = serde_json::from_str(&written).expect("a journal reads back");

    assert_eq!(
        read.epochs[0].corrections,
        some_corrections(),
        "written as {written}"
    );
    for ((_, _, read), (_, _, recorded)) in
        (read.epochs[0].corrections.iter()).zip(&some_corrections())
    {
        assert_eq!(read.factor.to_bits(), recorded.factor.to_bits());
        assert_eq!(read.spread.to_bits(), recorded.spread.to_bits());
    }
    assert!(
        read.epochs[1].corrections.is_empty(),
        "{:?}",
        read.epochs[1].corrections
    );
    assert_eq!(read, trace, "every epoch reads back as written");

    let mut long = journal(200);
    for epoch in &mut long.epochs {
        epoch.corrections = some_corrections();
    }
    let long = serde_json::to_string(&long).expect("a journal serialises");
    assert_eq!(long.matches(ALPHA.name()).count(), 1, "{ALPHA} once");
    assert_eq!(long.matches(BETA.name()).count(), 1, "{BETA} once");
}

/// **A journal written with a per-agent calibration still reads.** Before
/// each row was corrected, an epoch held one measured time per agent under
/// `calibration`, and no `corrections`. Such a journal reads back with no
/// correction, and everything else as written.
#[test]
fn an_older_journal_reads_its_calibration() {
    let mut trace = journal(2);
    for epoch in &mut trace.epochs {
        epoch.corrections = some_corrections();
    }
    let written = serde_json::to_string(&trace).expect("a journal serialises");
    let mut older: serde_json::Value = serde_json::from_str(&written).expect("a journal is JSON");
    for epoch in older["epochs"].as_array_mut().expect("the epochs") {
        let epoch = epoch.as_object_mut().expect("an epoch");
        epoch
            .remove("corrections")
            .unwrap_or_else(|| panic!("the corrections are written: {written}"));
        epoch.insert("calibration".to_owned(), serde_json::json!([[0, 2.5]]));
    }

    let read = serde_json::from_value::<DecisionTrace>(older).expect("an older journal reads");

    assert_eq!(read.epochs.len(), 2);
    for (read, written) in read.epochs.iter().zip(&trace.epochs) {
        assert!(read.corrections.is_empty(), "{:?}", read.corrections);
        assert_eq!(read.allocation, written.allocation);
        assert_eq!(read.offers, written.offers);
        assert_eq!(read.reports, written.reports);
        assert_eq!(read.why, written.why);
        assert_eq!(read.planning, written.planning);
    }
}

// ── The frame report ──────────────────────────────────────────────

/// A frame report measuring `cpu`, `gpu`, `npu` and `other`, each in
/// milliseconds; `None` where the frame did not measure the device.
fn frame(cpu: Option<u64>, gpu: Option<u64>, npu: Option<u64>, other: Option<u64>) -> FrameReport {
    FrameReport {
        busy: PerDevice {
            cpu: cpu.map(Duration::from_millis),
            gpu: gpu.map(Duration::from_millis),
            npu: npu.map(Duration::from_millis),
            other: other.map(Duration::from_millis),
        },
    }
}

/// **A frame that measured no device has no critical path.** "Not measured"
/// is not zero: the report says nothing of the frame's time.
#[test]
fn a_frame_measuring_no_device_has_no_critical_path() {
    assert_eq!(FrameReport::default().critical(), None);
    assert_eq!(frame(None, None, None, None).critical(), None);
}

/// **The critical path is the slowest measured device.** The devices work
/// side by side: the frame lasts as long as the busiest one, whichever it is.
#[test]
fn the_critical_path_is_the_slowest_measured_device() {
    let ms = Duration::from_millis;
    assert_eq!(frame(Some(5), None, None, None).critical(), Some(ms(5)));
    assert_eq!(
        frame(Some(5), Some(40), None, None).critical(),
        Some(ms(40))
    );
    assert_eq!(
        frame(Some(40), Some(5), None, None).critical(),
        Some(ms(40))
    );
    assert_eq!(
        frame(Some(3), Some(7), Some(12), Some(9)).critical(),
        Some(ms(12)),
        "the max, not the sum"
    );
    assert_eq!(frame(None, None, None, Some(8)).critical(), Some(ms(8)));
}

/// **An unmeasured device never wins over a measured one.** A frame that
/// measured its host at zero and nothing else lasted zero on its critical
/// path, not "unknown"; one that measured only its GPU lasts the GPU's time.
#[test]
fn an_unmeasured_device_never_wins_over_a_measured_one() {
    assert_eq!(
        frame(Some(0), None, None, None).critical(),
        Some(Duration::ZERO),
        "a measured zero is a time"
    );
    assert_eq!(
        frame(None, Some(4), None, None).critical(),
        Some(Duration::from_millis(4))
    );
}
