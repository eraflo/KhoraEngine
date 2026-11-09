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
    Allocation, Confidence, Envelope, Epoch, FrameReport, Offer, OfferRow, Report, Trigger,
};
use super::{AdaptationMode, AgentHints, AgentId, DecisionTrace, EpochRecord, StrategyId};
use crate::lane::cost::{Cost, PerDevice};

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
