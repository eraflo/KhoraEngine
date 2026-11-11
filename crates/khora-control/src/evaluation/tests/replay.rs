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

//! A journal decided again: through the arbiter that recorded it, it gives
//! back what was recorded; through two proposer sets, it compares them; and
//! whatever the set, it reads the quotes' error and the overruns from the
//! journal itself. Every figure is in whole microseconds.

use std::sync::{Arc, Mutex};

use super::*;
use crate::{DccConfig, DccService};
use khora_core::agent::gorna::protocol::{
    Allocation, Confidence, DecidedBy, Envelope, Epoch, FrameReport, Offer, OfferRequest, OfferRow,
    Report, Trigger, Why,
};
use khora_core::agent::gorna::{
    AdaptationMode, AgentId, Correction, DecisionTrace, EngineHint, EpochRecord, FrameSummary,
    Planning,
};
use khora_core::agent::{Agent, AgentImportance, ExecutionPhase, ExecutionTiming, Place};
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::telemetry::TelemetryEvent;

use AgentImportance::{Critical, Important, Optional};
use StrategyId::{Balanced, HighPerformance, LowPower};

/// Two weighed qualities closer than this are equal: qualities are `f32`,
/// read as `f64`.
const EPS: f64 = 1e-6;

const MIB: u64 = 1024 * 1024;

fn us(n: u64) -> Duration {
    Duration::from_micros(n)
}

/// A row of `cpu_us` on the host, of `quality`.
fn row(id: StrategyId, cpu_us: u64, quality: f32) -> OfferRow {
    OfferRow {
        id,
        cost: Cost {
            time: PerDevice {
                cpu: us(cpu_us),
                ..PerDevice::default()
            },
            ..Cost::default()
        },
        quality,
        confidence: Confidence::Measured,
    }
}

/// `name`'s offer for `epoch`: `rows`, the first its floor, running
/// `current`.
fn offer(name: &'static str, epoch: u64, rows: Vec<OfferRow>, current: StrategyId) -> Offer {
    Offer {
        agent: AgentId::new(name),
        epoch: Epoch(epoch),
        floor: rows[0].id,
        rows,
        current,
        health: 1.0,
    }
}

/// A journal entry for epoch `epoch`, opened at `at`, sharing `target_us`
/// among `offers` weighed by `importance`: no policy, hint, wave, memory
/// limit, background agent nor correction. Its allocation is a stand-in:
/// these tests read what the epoch is decided again into, not what it was.
fn epoch(
    epoch: u64,
    at: Duration,
    target_us: u64,
    offers: Vec<Offer>,
    importance: Vec<(AgentId, AgentImportance)>,
) -> EpochRecord {
    EpochRecord {
        allocation: Allocation {
            epoch: Epoch(epoch),
            trigger: Trigger::Refresh,
            envelopes: Vec::new(),
            pins: Vec::new(),
            allowed: Vec::new(),
            price: None,
            lever: false,
            late: Vec::new(),
        },
        target: us(target_us),
        offers,
        reports: Vec::new(),
        waves: Vec::new(),
        policies: Vec::new(),
        hints: Vec::new(),
        reasons: Vec::new(),
        why: Why {
            trigger: Trigger::Refresh,
            ..Why::default()
        },
        importance,
        corrections: Vec::new(),
        memory: PerDevice::default(),
        background: Vec::new(),
        at,
        frames: FrameSummary::default(),
        in_force: Vec::new(),
        planning: Planning::default(),
        planned: Vec::new(),
    }
}

/// `replayed`'s planned rows, by agent name.
fn rows_of(replayed: &ReplayedEpoch) -> Vec<(&'static str, StrategyId)> {
    let mut rows: Vec<(&'static str, StrategyId)> = (replayed.rows.iter())
        .map(|(agent, row)| (agent.name(), *row))
        .collect();
    rows.sort_by_key(|(name, _)| *name);
    rows
}

// ── Two proposer sets on one journal ─────────────────────────────

/// Three epochs of one knapsack round: `oak` (Critical, weight 1.0) offers
/// 1 ms at 0.4 or 3.9 ms at 1.0, `pine` (Optional, weight 0.5) 1 ms at 0.4
/// or 1.5 ms at 1.0, for a 5 ms target. The Q-RAM pass upgrades `pine`
/// (0.9 weighed, 2.5 ms planned); the optimum upgrades `oak` instead (1.2
/// weighed, 4.9 ms planned).
fn knapsack_journal() -> DecisionTrace {
    let importance = vec![
        (AgentId::new("oak"), Critical),
        (AgentId::new("pine"), Optional),
    ];
    DecisionTrace {
        epochs: (1..=3)
            .map(|n| {
                epoch(
                    n,
                    Duration::from_secs(n),
                    5_000,
                    vec![
                        offer(
                            "oak",
                            n,
                            vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 3_900, 1.0)],
                            LowPower,
                        ),
                        offer(
                            "pine",
                            n,
                            vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 1_500, 1.0)],
                            LowPower,
                        ),
                    ],
                    importance.clone(),
                )
            })
            .collect(),
    }
}

/// **Replay compares the proposer sets on one journal.** On the knapsack
/// journal, the Q-RAM pass alone plans 0.9 of weighed quality an epoch and
/// the exact search after it 1.2: the second set's mean quality is the
/// greater, each epoch says which proposer decided it, both plans fit the
/// target — 2.5 ms and 0.1 ms of slack — and the shield refuses neither.
#[test]
fn replay_compares_the_arbiters_on_one_journal() {
    let trace = knapsack_journal();

    let greedy = replay(&trace, Arbiters::Greedy);
    let exact = replay(&trace, Arbiters::GreedyExact);

    for (set, replayed) in [("greedy", &greedy), ("greedy+exact", &exact)] {
        assert_eq!(replayed.epochs.len(), 3, "{set}: every epoch decided again");
        assert_eq!(replayed.metrics.epochs, 3, "{set}");
        assert_eq!(replayed.metrics.refusals, 0, "{set}: no plan refused");
        assert!(
            replayed.metrics.slack_mean_ms >= 0.0,
            "{set}: the plans fit the target: {}",
            replayed.metrics.slack_mean_ms
        );
        for epoch in &replayed.epochs {
            assert!(epoch.refused.is_empty(), "{set}: {:?}", epoch.refused);
            assert!(epoch.slack_ms >= 0.0, "{set}: {}", epoch.slack_ms);
        }
        assert_eq!(
            replayed.metrics.row_changes_per_minute,
            Some(0.0),
            "{set}: one round three times plans the same rows"
        );
    }

    assert!(
        (greedy.metrics.quality_mean - 0.9).abs() < EPS,
        "the Q-RAM pass plans 0.9: {}",
        greedy.metrics.quality_mean
    );
    assert!(
        (exact.metrics.quality_mean - 1.2).abs() < EPS,
        "the exact search plans 1.2: {}",
        exact.metrics.quality_mean
    );
    assert!(exact.metrics.quality_mean > greedy.metrics.quality_mean);
    assert!(
        (greedy.metrics.slack_mean_ms - 2.5).abs() < 1e-3,
        "5 ms less 1 + 1.5 ms planned: {}",
        greedy.metrics.slack_mean_ms
    );
    assert!(
        (exact.metrics.slack_mean_ms - 0.1).abs() < 1e-3,
        "5 ms less 3.9 + 1 ms planned: {}",
        exact.metrics.slack_mean_ms
    );

    for epoch in &greedy.epochs {
        assert_eq!(epoch.why.by, DecidedBy::Greedy, "{:?}", epoch.why);
        assert!((epoch.quality - 0.9).abs() < EPS, "{}", epoch.quality);
        assert_eq!(
            rows_of(epoch),
            vec![("oak", LowPower), ("pine", HighPerformance)]
        );
    }
    for epoch in &exact.epochs {
        assert_eq!(epoch.why.by, DecidedBy::Exact, "{:?}", epoch.why);
        assert!((epoch.quality - 1.2).abs() < EPS, "{}", epoch.quality);
        assert_eq!(
            rows_of(epoch),
            vec![("oak", HighPerformance), ("pine", LowPower)]
        );
    }
}

// ── Row changes per minute ───────────────────────────────────────

/// Epochs opened at `times` (in seconds), each sharing the target of the
/// same rank in `targets_us` with `ash`, which offers 1 ms at 0.4 or 3 ms at
/// 1.0: a 10 ms target plans its upgrade, a 2 ms target its floor.
fn ash_journal(times: &[u64], targets_us: &[u64]) -> DecisionTrace {
    DecisionTrace {
        epochs: (times.iter().zip(targets_us).enumerate())
            .map(|(i, (&at, &target))| {
                let n = i as u64 + 1;
                epoch(
                    n,
                    Duration::from_secs(at),
                    target,
                    vec![offer(
                        "ash",
                        n,
                        vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 3_000, 1.0)],
                        LowPower,
                    )],
                    vec![(AgentId::new("ash"), Important)],
                )
            })
            .collect(),
    }
}

/// **Replay counts the row changes per minute from the epochs' times.**
/// Five epochs opened from 5 s to 65 s plan `ash`'s upgrade, its upgrade,
/// its floor, its floor, its upgrade: two changes over the sixty seconds
/// the epochs span, two a minute, whichever proposer set decides. A journal
/// whose epochs all opened at once, or of one epoch, spans no time: no rate.
#[test]
fn replay_counts_row_changes_per_minute_from_the_epochs_times() {
    let trace = ash_journal(
        &[5, 20, 35, 50, 65],
        &[10_000, 10_000, 2_000, 2_000, 10_000],
    );

    for arbiters in Arbiters::ALL {
        let replayed = replay(&trace, arbiters);
        let planned: Vec<StrategyId> = (replayed.epochs.iter())
            .map(|epoch| rows_of(epoch)[0].1)
            .collect();
        assert_eq!(
            planned,
            vec![
                HighPerformance,
                HighPerformance,
                LowPower,
                LowPower,
                HighPerformance
            ],
            "{arbiters:?}"
        );
        let rate = replayed
            .metrics
            .row_changes_per_minute
            .unwrap_or_else(|| panic!("{arbiters:?}: the epochs span a minute"));
        assert!(
            (rate - 2.0).abs() < 1e-9,
            "{arbiters:?}: two changes over sixty seconds: {rate}"
        );
    }

    let at_once = ash_journal(&[7, 7, 7], &[10_000, 2_000, 10_000]);
    assert_eq!(
        replay(&at_once, Arbiters::GreedyExact)
            .metrics
            .row_changes_per_minute,
        None,
        "epochs opened at once span no time"
    );
    let alone = ash_journal(&[7], &[10_000]);
    assert_eq!(
        replay(&alone, Arbiters::GreedyExact)
            .metrics
            .row_changes_per_minute,
        None,
        "one epoch spans no time"
    );
}

// ── What the journal says of itself ──────────────────────────────

/// A report that `ash` ran `row` under epoch `epoch`, quoting `quoted_us` on
/// the host (its workload) and taking `cpu_us` there.
fn ran(epoch: u64, row: StrategyId, quoted_us: u64, cpu_us: u64) -> Report {
    Report {
        agent: AgentId::new("ash"),
        epoch: Epoch(epoch),
        row,
        measured: PerDevice {
            cpu: us(cpu_us),
            ..PerDevice::default()
        },
        done: 1.0,
        health: 1.0,
        over_budget: false,
        stalled: false,
        workload: Some(quoted_us as f64),
        switch: None,
        message: None,
    }
}

/// `ash`'s rows both corrected by `factor`, with a spread of 0.1.
fn ash_corrected(factor: f64) -> Vec<(AgentId, StrategyId, Correction)> {
    let correction = Correction {
        factor,
        spread: 0.1,
    };
    vec![
        (AgentId::new("ash"), LowPower, correction),
        (AgentId::new("ash"), HighPerformance, correction),
    ]
}

/// Three epochs of `ash`, which offers 1 ms or 4 ms:
/// - epoch 1 corrects nothing; epoch 2's report says `ash` ran its 4 ms row
///   under it while its load had grown, quoting 4.4 ms for a run of 5.5 ms
///   — off by a fifth;
/// - epoch 2 corrects both rows ×1.5; epoch 3's report says `ash` ran the
///   1 ms row under it, quoting 0.9 ms — 1.35 ms corrected — for a run of
///   1 ms: off by 0.35;
/// - epoch 3 has no epoch after it.
///
/// What a run quoted is its own workload, not the offer the epoch read: the
/// offers quote 4 ms and 1 ms.
///
/// Their frames: 10 with 2 over, the worst 30 ms; 20 with none over, the
/// worst 12 ms; 5 all over, the worst 40 ms.
fn quoted_journal() -> DecisionTrace {
    let rows = || vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 4_000, 1.0)];
    let ash = AgentId::new("ash");
    let mut first = epoch(
        1,
        Duration::from_secs(1),
        10_000,
        vec![offer("ash", 1, rows(), LowPower)],
        vec![(ash, Important)],
    );
    first.frames = FrameSummary {
        count: 10,
        over_target: 2,
        worst: Duration::from_millis(30),
        mean: Duration::from_millis(15),
    };
    let mut second = epoch(
        2,
        Duration::from_secs(2),
        10_000,
        vec![offer("ash", 2, rows(), HighPerformance)],
        vec![(ash, Important)],
    );
    second.corrections = ash_corrected(1.5);
    second.reports = vec![ran(1, HighPerformance, 4_400, 5_500)];
    second.frames = FrameSummary {
        count: 20,
        over_target: 0,
        worst: Duration::from_millis(12),
        mean: Duration::from_millis(9),
    };
    let mut third = epoch(
        3,
        Duration::from_secs(3),
        10_000,
        vec![offer("ash", 3, rows(), LowPower)],
        vec![(ash, Important)],
    );
    third.reports = vec![ran(2, LowPower, 900, 1_000)];
    third.frames = FrameSummary {
        count: 5,
        over_target: 5,
        worst: Duration::from_millis(40),
        mean: Duration::from_millis(35),
    };
    DecisionTrace {
        epochs: vec![first, second, third],
    }
}

/// **Replay reads the prediction error and the overruns from the journal.**
/// Each run is paired with the epoch it ran under: what it quoted, corrected
/// as that epoch corrected its row, against its host time. Off by a fifth,
/// then — corrected — by 0.35: a mean error of 0.275. The frames number 35,
/// 7 of them over their target, the worst 40 ms. Neither depends on the
/// proposer set; a journal with no report has no error to read.
#[test]
fn replay_reads_the_prediction_error_and_overruns_from_the_journal() {
    let trace = quoted_journal();

    for arbiters in Arbiters::ALL {
        let metrics = replay(&trace, arbiters).metrics;
        let error = metrics
            .prediction_error
            .unwrap_or_else(|| panic!("{arbiters:?}: two quotes meet their run"));
        assert!(
            (error - 0.275).abs() < 1e-9,
            "{arbiters:?}: (0.2 + 0.35) / 2: {error}"
        );
        assert_eq!(metrics.frames, 35, "{arbiters:?}");
        assert_eq!(metrics.frames_over, 7, "{arbiters:?}");
        assert_eq!(
            metrics.worst_frame,
            Duration::from_millis(40),
            "{arbiters:?}"
        );
    }

    let unreported = replay(&knapsack_journal(), Arbiters::GreedyExact).metrics;
    assert_eq!(unreported.prediction_error, None, "no run reported");
    assert_eq!(unreported.frames, 0);
    assert_eq!(unreported.frames_over, 0);
    assert_eq!(unreported.worst_frame, Duration::ZERO);
}

// ── A journal recorded by the DCC ────────────────────────────────

/// An agent offering fixed rows, of a fixed importance, at a fixed phase:
/// it runs its floor and reports it.
struct Fixed {
    id: AgentId,
    rows: Vec<OfferRow>,
    importance: AgentImportance,
    phase: ExecutionPhase,
    place: Arc<Mutex<Place>>,
}

impl Fixed {
    fn agent(
        name: &'static str,
        rows: Vec<OfferRow>,
        importance: AgentImportance,
        phase: ExecutionPhase,
    ) -> Arc<Mutex<dyn Agent>> {
        Arc::new(Mutex::new(Fixed {
            id: AgentId::new(name),
            rows,
            importance,
            phase,
            place: Arc::new(Mutex::new(Place::new(AgentId::new(name)))),
        }))
    }
}

impl Agent for Fixed {
    fn id(&self) -> AgentId {
        self.id
    }

    fn offer(&mut self, request: &OfferRequest) -> Offer {
        Offer {
            agent: self.id,
            epoch: request.epoch,
            rows: self.rows.clone(),
            floor: self.rows[0].id,
            current: self.rows[0].id,
            health: 1.0,
        }
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
        Report {
            agent: self.id,
            epoch: Epoch::default(),
            row: self.rows[0].id,
            measured: self.rows[0].cost.time,
            done: 1.0,
            health: 1.0,
            over_budget: false,
            stalled: false,
            workload: None,
            switch: None,
            message: None,
        }
    }

    fn place(&self) -> Arc<Mutex<Place>> {
        Arc::clone(&self.place)
    }

    fn execution_timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            importance: self.importance,
            default_phase: self.phase,
            ..ExecutionTiming::default()
        }
    }
}

/// A frame whose host was busy `ms` milliseconds.
fn frame(ms: u64) -> TelemetryEvent {
    TelemetryEvent::FrameReport(FrameReport {
        busy: PerDevice {
            cpu: Some(Duration::from_millis(ms)),
            ..PerDevice::default()
        },
    })
}

/// Waits up to `secs` seconds for `done`, sending `event` every 5 ms.
fn feed_until(
    tx: &crossbeam_channel::Sender<TelemetryEvent>,
    secs: u64,
    event: impl Fn() -> TelemetryEvent,
    mut done: impl FnMut() -> bool,
) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    while std::time::Instant::now() < deadline {
        if done() {
            return true;
        }
        tx.send(event()).expect("the DCC listens");
        std::thread::sleep(Duration::from_millis(5));
    }
    done()
}

/// A journal the running DCC recorded, with every input a round reads:
/// - `oak` (Critical) offers three rows, the best holding 300 MiB of video
///   memory where 256 MiB are left, and is capped at 8 ms; a report of a
///   3 ms run of its 2 ms floor teaches the cost book that floor's
///   correction;
/// - `pine` (Optional) is `Stable`: it keeps the envelope it was lent;
/// - `elm` (Important) runs at the background.
///
/// Frames of 40 ms pull the target down for a few epochs, then frames of
/// 5 ms let it rise again.
fn recorded_journal() -> DecisionTrace {
    let (mut dcc, rx) = DccService::new(DccConfig {
        tick_rate: 100,
        ..Default::default()
    });
    dcc.context_handle()
        .write()
        .expect("the context lock")
        .hardware
        .available_vram = Some(256 * MIB);

    let mut best = row(HighPerformance, 9_000, 1.0);
    best.cost.memory.gpu = 300 * MIB;
    dcc.register_agent(
        Fixed::agent(
            "oak",
            vec![row(LowPower, 2_000, 0.4), row(Balanced, 5_000, 0.7), best],
            Critical,
            ExecutionPhase::OUTPUT,
        ),
        1.0,
    );
    dcc.register_agent(
        Fixed::agent(
            "pine",
            vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 3_000, 1.0)],
            Optional,
            ExecutionPhase::OUTPUT,
        ),
        1.0,
    );
    dcc.register_agent(
        Fixed::agent(
            "elm",
            vec![row(LowPower, 4_000, 0.5), row(HighPerformance, 12_000, 1.0)],
            Important,
            ExecutionPhase::BACKGROUND,
        ),
        1.0,
    );
    dcc.set_adaptation_mode(AgentId::new("pine"), AdaptationMode::Stable);
    dcc.set_hint(EngineHint::Cap {
        agent: AgentId::new("oak"),
        max_ms: 8.0,
    });
    let tx = dcc.event_sender();
    tx.send(TelemetryEvent::AgentReport(Report {
        agent: AgentId::new("oak"),
        epoch: Epoch(0),
        row: LowPower,
        measured: PerDevice {
            cpu: us(3_000),
            ..PerDevice::default()
        },
        done: 1.0,
        health: 1.0,
        over_budget: false,
        stalled: false,
        // The host time its floor quoted, in µs.
        workload: Some(2_000.0),
        switch: None,
        message: None,
    }))
    .expect("the DCC listens");
    dcc.start_decision_recording();
    dcc.start(rx);

    let recorded = |dcc: &DccService| dcc.recorded_decisions().epochs.len();
    feed_until(&tx, 10, || frame(40), || recorded(&dcc) >= 3);
    let squeezed = recorded(&dcc);
    feed_until(&tx, 5, || frame(5), || recorded(&dcc) >= squeezed + 2);
    let trace = dcc.stop_decision_recording();
    dcc.stop();
    trace
}

/// **A journal replayed through its arbiter decides what it recorded.** The
/// running DCC records epochs whose rounds read importance, a correction,
/// a memory limit, an agent at the background, a `Stable` policy, a cap
/// and a target the governor moved. Decided again from the journal alone
/// by the engine's own proposer set, every epoch gives back the allocation
/// recorded, and why.
#[test]
fn a_journal_replayed_through_its_arbiter_decides_what_it_recorded() {
    let trace = recorded_journal();
    assert!(
        trace.epochs.len() >= 3,
        "the DCC recorded a few epochs: {:?}",
        trace
            .epochs
            .iter()
            .map(|e| &e.allocation)
            .collect::<Vec<_>>()
    );

    let replayed = replay(&trace, Arbiters::Planned);

    assert_eq!(replayed.epochs.len(), trace.epochs.len());
    for (again, recorded) in replayed.epochs.iter().zip(&trace.epochs) {
        assert_eq!(
            again.allocation, recorded.allocation,
            "epoch {:?} decided again",
            recorded.allocation.epoch
        );
        assert_eq!(
            again.why, recorded.why,
            "epoch {:?} explained again",
            recorded.allocation.epoch
        );
    }
    assert_eq!(
        replayed.metrics.same_as_recorded,
        trace.epochs.len(),
        "every epoch counted as given back"
    );
}

/// **A journal begun while the DCC runs decides its first epoch again.**
/// The DCC decides a few epochs before a journal begins, so the journal's
/// first round opens with an allocation in force that it does not hold:
/// `pine`, `Stable`, keeps the envelope it was lent then. Decided again
/// from the journal alone, that first epoch gives back the allocation
/// recorded, as every other one does.
#[test]
fn a_journal_begun_while_the_dcc_runs_decides_its_first_epoch_again() {
    let (mut dcc, rx) = DccService::new(DccConfig {
        tick_rate: 100,
        ..Default::default()
    });
    dcc.register_agent(
        Fixed::agent(
            "oak",
            vec![row(LowPower, 2_000, 0.4), row(Balanced, 5_000, 0.7)],
            Critical,
            ExecutionPhase::OUTPUT,
        ),
        1.0,
    );
    dcc.register_agent(
        Fixed::agent(
            "pine",
            vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 3_000, 1.0)],
            Optional,
            ExecutionPhase::OUTPUT,
        ),
        1.0,
    );
    dcc.set_adaptation_mode(AgentId::new("pine"), AdaptationMode::Stable);
    let tx = dcc.event_sender();
    // A first journal only to see the epochs the DCC opens before the one
    // under test begins.
    dcc.start_decision_recording();
    dcc.start(rx);
    let recorded = |dcc: &DccService| dcc.recorded_decisions().epochs.len();
    feed_until(&tx, 10, || frame(40), || recorded(&dcc) >= 3);
    assert!(
        recorded(&dcc) >= 3,
        "the DCC decided before the journal began"
    );

    // The journal under test begins with an allocation in force; frames
    // with room let the target rise again.
    dcc.start_decision_recording();
    feed_until(&tx, 10, || frame(5), || recorded(&dcc) >= 2);
    let trace = dcc.stop_decision_recording();
    dcc.stop();
    assert!(!trace.epochs.is_empty(), "the journal recorded epochs");

    let replayed = replay(&trace, Arbiters::Planned);

    for (again, recorded) in replayed.epochs.iter().zip(&trace.epochs) {
        assert_eq!(
            again.allocation, recorded.allocation,
            "epoch {:?} decided again",
            recorded.allocation.epoch
        );
    }
    assert_eq!(replayed.metrics.same_as_recorded, trace.epochs.len());
}

/// **The prediction error pairs a run with the quote of the epoch it ran
/// in.** A journal keeps each agent's latest report when a round opens, so
/// an agent that ran once, under epoch 1, is reported at epoch 2 and again
/// at epoch 3. Its run met its quote exactly under epoch 1, which corrected
/// nothing; epoch 2 corrected the same row ×3, but `ash` never ran under it.
/// The error is the one run against the correction it ran under: none.
#[test]
fn the_prediction_error_pairs_a_run_with_the_quote_of_the_epoch_it_ran_in() {
    let rows = || vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 4_000, 1.0)];
    let ash = AgentId::new("ash");
    let first = epoch(
        1,
        Duration::from_secs(1),
        10_000,
        vec![offer("ash", 1, rows(), LowPower)],
        vec![(ash, Important)],
    );
    let mut second = epoch(
        2,
        Duration::from_secs(2),
        10_000,
        vec![offer("ash", 2, rows(), LowPower)],
        vec![(ash, Important)],
    );
    second.corrections = ash_corrected(3.0);
    second.reports = vec![ran(1, LowPower, 1_000, 1_000)];
    let mut third = epoch(
        3,
        Duration::from_secs(3),
        10_000,
        vec![offer("ash", 3, rows(), LowPower)],
        vec![(ash, Important)],
    );
    // No run since: the latest report is still the one of epoch 1.
    third.reports = vec![ran(1, LowPower, 1_000, 1_000)];
    let trace = DecisionTrace {
        epochs: vec![first, second, third],
    };

    let error = replay(&trace, Arbiters::GreedyExact)
        .metrics
        .prediction_error
        .expect("one run meets its quote");

    assert!(
        error.abs() < 1e-9,
        "the run under epoch 1 met epoch 1's quote; it is counted once: {error}"
    );
}

/// One epoch of `ash`, which offers 1 ms at 0.4 or 3 ms at 1.0 for a 5 ms
/// target, its rows corrected by `factor` with `spread`, its round planning
/// `caution` spreads above each correction and nothing else.
fn corrected_journal(factor: f64, spread: f64, caution: f64) -> DecisionTrace {
    let ash = AgentId::new("ash");
    let mut record = epoch(
        1,
        Duration::from_secs(1),
        5_000,
        vec![offer(
            "ash",
            1,
            vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 3_000, 1.0)],
            LowPower,
        )],
        vec![(ash, Important)],
    );
    let correction = Correction { factor, spread };
    record.corrections = vec![
        (ash, LowPower, correction),
        (ash, HighPerformance, correction),
    ];
    record.planning = Planning {
        caution,
        ..Planning::default()
    };
    DecisionTrace {
        epochs: vec![record],
    }
}

/// `ash`'s planned row when `trace` is decided again through `arbiters`.
fn ash_planned(trace: &DecisionTrace, arbiters: Arbiters) -> StrategyId {
    let replayed = replay(trace, arbiters);
    rows_of(&replayed.epochs[0])[0].1
}

/// **A replay decides on the corrections its journal recorded.**
/// - `ash`'s rows ran twice what they quote: its 3 ms upgrade is 6 ms, over
///   the 5 ms target, for every set that reads the corrections — the Q-RAM
///   pass, the exact search after it, the planned set — and fits for the
///   planned set on raw quotes.
/// - Corrected ×1.5 with a spread of 0.5, under a round that planned one
///   spread of caution: the planned set plans the upgrade at 3 × 2 = 6 ms and
///   keeps the floor; the Q-RAM pass and the exact search, planning no
///   caution, plan it at 4.5 ms and take it.
#[test]
fn a_replay_decides_on_the_corrections_its_journal_recorded() {
    let doubled = corrected_journal(2.0, 0.0, 0.0);
    for arbiters in [Arbiters::Greedy, Arbiters::GreedyExact, Arbiters::Planned] {
        assert_eq!(
            ash_planned(&doubled, arbiters),
            LowPower,
            "{arbiters:?}: the upgrade corrected is 6 ms, over 5 ms"
        );
    }
    assert_eq!(
        ash_planned(&doubled, Arbiters::PlannedOnQuotes),
        HighPerformance,
        "on raw quotes, the 3 ms upgrade fits 5 ms"
    );

    let uncertain = corrected_journal(1.5, 0.5, 1.0);
    assert_eq!(
        ash_planned(&uncertain, Arbiters::Planned),
        LowPower,
        "planned with its caution, the upgrade is 3 × (1.5 + 0.5) = 6 ms"
    );
    for arbiters in [Arbiters::Greedy, Arbiters::GreedyExact] {
        assert_eq!(
            ash_planned(&uncertain, arbiters),
            HighPerformance,
            "{arbiters:?}: no caution, the upgrade is 3 × 1.5 = 4.5 ms"
        );
    }
}

/// **A journal with an agent busy through its rounds decides what it
/// recorded.** `pine`'s lock is held through a few rounds, so the DCC
/// finds it busy: it keeps the envelope it holds, named late, with no
/// offer. Decided again from the journal alone, every epoch — those with
/// `pine` busy included — gives back the allocation recorded.
#[test]
fn a_journal_with_an_agent_busy_through_its_rounds_decides_what_it_recorded() {
    let (mut dcc, rx) = DccService::new(DccConfig {
        tick_rate: 100,
        ..Default::default()
    });
    let pine = Fixed::agent(
        "pine",
        vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 3_000, 1.0)],
        Optional,
        ExecutionPhase::OUTPUT,
    );
    dcc.register_agent(
        Fixed::agent(
            "oak",
            vec![row(LowPower, 2_000, 0.4), row(Balanced, 5_000, 0.7)],
            Critical,
            ExecutionPhase::OUTPUT,
        ),
        1.0,
    );
    dcc.register_agent(Arc::clone(&pine), 1.0);
    let tx = dcc.event_sender();
    dcc.start_decision_recording();
    dcc.start(rx);
    let recorded = |dcc: &DccService| dcc.recorded_decisions().epochs.len();
    feed_until(&tx, 10, || frame(40), || recorded(&dcc) >= 2);
    let before_busy = recorded(&dcc);
    {
        let _busy = pine.lock().expect("pine's lock");
        feed_until(&tx, 10, || frame(5), || recorded(&dcc) >= before_busy + 2);
    }
    let trace = dcc.stop_decision_recording();
    dcc.stop();
    assert!(
        (trace.epochs.iter()).any(|epoch| epoch.allocation.late.contains(&AgentId::new("pine"))),
        "some round found pine busy: {:?}",
        trace
            .epochs
            .iter()
            .map(|e| &e.allocation)
            .collect::<Vec<_>>()
    );

    let replayed = replay(&trace, Arbiters::Planned);

    for (again, recorded) in replayed.epochs.iter().zip(&trace.epochs) {
        assert_eq!(
            again.allocation, recorded.allocation,
            "epoch {:?} decided again",
            recorded.allocation.epoch
        );
        assert_eq!(again.why, recorded.why);
    }
}

/// **A journal's `Safety` rounds decide again as recorded.** One round
/// opened by the governor whose floors do not fit the target, one opened as
/// `Safety` (a death spiral) whose floors fit: both lend every agent its
/// floor, the first naming the breach, the second none. Each is recorded
/// as the engine's arbiter decides it, and decided again the same.
#[test]
fn a_journals_safety_rounds_decide_again_as_recorded() {
    let offers = |n: u64| {
        vec![
            offer(
                "oak",
                n,
                vec![row(LowPower, 3_000, 0.4), row(HighPerformance, 6_000, 1.0)],
                LowPower,
            ),
            offer(
                "pine",
                n,
                vec![row(LowPower, 2_000, 0.4), row(HighPerformance, 4_000, 1.0)],
                LowPower,
            ),
        ]
    };
    let importance = vec![
        (AgentId::new("oak"), Critical),
        (AgentId::new("pine"), Optional),
    ];
    let mut trace = DecisionTrace {
        epochs: vec![
            epoch(
                1,
                Duration::from_secs(1),
                4_000,
                offers(1),
                importance.clone(),
            ),
            epoch(2, Duration::from_secs(2), 20_000, offers(2), importance),
        ],
    };
    let arbiter = Arbiters::GreedyExact.arbiter();
    for (record, trigger) in trace
        .epochs
        .iter_mut()
        .zip([Trigger::FrameGovernor, Trigger::Safety])
    {
        let bids: Vec<crate::gorna::Bid> = (record.offers.iter())
            .map(|offer| crate::gorna::Bid {
                offer: offer.clone(),
                importance: if offer.agent == AgentId::new("oak") {
                    Critical
                } else {
                    Optional
                },
                mode: AdaptationMode::Learning,
                hints: Default::default(),
                kept: None,
                late: false,
            })
            .collect();
        let decision = arbiter.decide(&crate::gorna::Round {
            epoch: record.allocation.epoch,
            trigger,
            target: record.target,
            memory: PerDevice::default(),
            link_rate: Default::default(),
            waves: &[],
            background: &[],
            bids: &bids,
            planning: Planning::default(),
        });
        record.allocation = decision.allocation;
        record.why = decision.why;
    }
    assert!(trace.epochs[0].why.binding.is_some(), "the floors breach");
    assert!(trace.epochs[1].why.binding.is_none(), "opened as Safety");

    let replayed = replay(&trace, Arbiters::GreedyExact);

    for (again, recorded) in replayed.epochs.iter().zip(&trace.epochs) {
        assert_eq!(again.allocation, recorded.allocation);
        assert_eq!(again.why, recorded.why);
    }
}

/// **A journal with a replay between two of its epochs decides what it
/// recorded.** While the DCC records, it replays an earlier allocation: the
/// next live round opens with that one in force, not with the journal's
/// previous epoch. `pine`, `Stable`, keeps what the replay lent it; decided
/// again from the journal alone, every epoch gives back the allocation
/// recorded.
#[test]
fn a_journal_with_a_replay_between_its_epochs_decides_what_it_recorded() {
    let (mut dcc, rx) = DccService::new(DccConfig {
        tick_rate: 100,
        ..Default::default()
    });
    dcc.register_agent(
        Fixed::agent(
            "oak",
            vec![row(LowPower, 2_000, 0.4), row(Balanced, 5_000, 0.7)],
            Critical,
            ExecutionPhase::OUTPUT,
        ),
        1.0,
    );
    dcc.register_agent(
        Fixed::agent(
            "pine",
            vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 3_000, 1.0)],
            Optional,
            ExecutionPhase::OUTPUT,
        ),
        1.0,
    );
    dcc.set_adaptation_mode(AgentId::new("pine"), AdaptationMode::Stable);
    let tx = dcc.event_sender();
    dcc.start_decision_recording();
    dcc.start(rx);
    let recorded = |dcc: &DccService| dcc.recorded_decisions().epochs.len();
    feed_until(&tx, 10, || frame(40), || recorded(&dcc) >= 3);
    let before = dcc.recorded_decisions();
    assert!(before.epochs.len() >= 3, "the DCC decided a few epochs");

    // The first allocation, replayed between two live epochs.
    dcc.replay_decisions(DecisionTrace {
        epochs: vec![before.epochs[0].clone()],
    });
    let after = before.epochs.len();
    feed_until(&tx, 10, || frame(5), || recorded(&dcc) >= after + 2);
    let trace = dcc.stop_decision_recording();
    dcc.stop();
    assert!(trace.epochs.len() > after, "live epochs after the replay");
    assert!(
        (trace.epochs.windows(2)).any(|pair| pair[1].in_force != pair[0].allocation.envelopes),
        "a round opened with the replayed allocation in force"
    );

    let replayed = replay(&trace, Arbiters::Planned);

    for (again, recorded) in replayed.epochs.iter().zip(&trace.epochs) {
        assert_eq!(
            again.allocation, recorded.allocation,
            "epoch {:?} decided again",
            recorded.allocation.epoch
        );
    }
}

// ── A journal the DCC planned ────────────────────────────────────

/// `oak`'s report of a run of `row`, under epoch 0, quoting `quoted_us` on
/// the host — its workload — and taking `factor` times that, carrying
/// `switch`.
fn oak_ran(
    row: StrategyId,
    quoted_us: f64,
    factor: f64,
    switch: Option<Duration>,
) -> TelemetryEvent {
    TelemetryEvent::AgentReport(Report {
        agent: AgentId::new("oak"),
        epoch: Epoch(0),
        row,
        measured: PerDevice {
            cpu: Duration::from_secs_f64(quoted_us * factor * 1e-6),
            ..PerDevice::default()
        },
        done: 1.0,
        health: 1.0,
        over_budget: false,
        stalled: false,
        workload: Some(quoted_us),
        switch,
        message: None,
    })
}

/// A journal the running DCC recorded while planning beyond the frame:
/// - `oak` (Critical) offers 2 ms, 5 ms or 9 ms; it measured once, under
///   epoch 0, a switch to `Balanced` of 3 ms — that first run teaches the
///   cost book nothing — and ran `Balanced` once more for what it quoted;
///   then it runs its floor, one report in four events, each run taking
///   twice the host time it quoted, its quote rising slowly — each row its
///   own correction;
/// - `pine` (Optional) offers 1 ms or 3 ms and reports nothing.
///
/// The frames jitter around 40 ms for a few epochs, so the margin has a
/// spread to read, then around 5 ms until at least seven epochs are
/// recorded.
fn planned_journal() -> DecisionTrace {
    let (mut dcc, rx) = DccService::new(DccConfig {
        tick_rate: 100,
        ..Default::default()
    });
    dcc.register_agent(
        Fixed::agent(
            "oak",
            vec![
                row(LowPower, 2_000, 0.4),
                row(Balanced, 5_000, 0.7),
                row(HighPerformance, 9_000, 1.0),
            ],
            Critical,
            ExecutionPhase::OUTPUT,
        ),
        1.0,
    );
    dcc.register_agent(
        Fixed::agent(
            "pine",
            vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 3_000, 1.0)],
            Optional,
            ExecutionPhase::OUTPUT,
        ),
        1.0,
    );
    let tx = dcc.event_sender();
    tx.send(oak_ran(Balanced, 5_000.0, 1.0, Some(us(3_000))))
        .expect("the DCC listens");
    tx.send(oak_ran(Balanced, 5_000.0, 1.0, None))
        .expect("the DCC listens");
    dcc.start_decision_recording();
    dcc.start(rx);

    let sent = std::cell::Cell::new(0_u64);
    let jittered = |around: u64| {
        let k = sent.get();
        sent.set(k + 1);
        if k % 4 == 3 {
            oak_ran(LowPower, 2_000.0 + k as f64 / 10.0, 2.0, None)
        } else {
            frame(around - 2 + 2 * (k % 3))
        }
    };
    let recorded = |dcc: &DccService| dcc.recorded_decisions().epochs.len();
    feed_until(&tx, 10, || jittered(40), || recorded(&dcc) >= 3);
    feed_until(&tx, 15, || jittered(5), || recorded(&dcc) >= 7);
    let trace = dcc.stop_decision_recording();
    dcc.stop();
    trace
}

/// **A journal replayed through the planned arbiter decides what it
/// recorded.** The running DCC plans its rounds beyond the frame — a switch
/// measured, a margin from the frames' spread, a rising workload — on the
/// rows its cost book corrected: `oak`'s floor runs twice what it quotes,
/// its `Balanced` what it quotes. It records each round's planning and each
/// row's correction. Decided again from the journal alone by the planned
/// set, every epoch gives back the allocation recorded, and why.
#[test]
fn a_journal_replayed_through_the_planned_arbiter_decides_what_it_recorded() {
    let trace = planned_journal();
    let corrected = |epoch: &EpochRecord, row: StrategyId| {
        (epoch.corrections.iter())
            .find(|(agent, named, _)| *agent == AgentId::new("oak") && *named == row)
            .map(|(_, _, correction)| correction.factor)
    };
    // `Balanced`, unrun while the floor runs, relaxes toward the floor's
    // correction as the session goes on: it is read on the first epoch.
    let (first, last) = (
        trace.epochs.first().expect("the DCC recorded epochs"),
        trace.epochs.last().expect("the DCC recorded epochs"),
    );
    assert!(
        corrected(last, LowPower).is_some_and(|factor| (factor - 2.0).abs() < 0.1),
        "oak's floor ran twice its quote: {:?}",
        last.corrections
    );
    assert!(
        corrected(first, Balanced).is_some_and(|factor| (factor - 1.0).abs() < 0.1),
        "oak's Balanced ran what it quoted: {:?}",
        first.corrections
    );
    assert!(
        trace.epochs.len() >= 5,
        "the DCC recorded a few epochs: {:?}",
        (trace.epochs.iter())
            .map(|e| &e.allocation)
            .collect::<Vec<_>>()
    );
    assert!(
        trace
            .epochs
            .iter()
            .any(|epoch| !epoch.planning.switch.is_empty()),
        "some round charged a switch: {:?}",
        (trace.epochs.iter())
            .map(|e| &e.planning)
            .collect::<Vec<_>>()
    );
    assert!(
        (trace.epochs.iter()).any(|epoch| epoch.planning.margin > Duration::ZERO),
        "some round kept a margin: {:?}",
        (trace.epochs.iter())
            .map(|e| &e.planning)
            .collect::<Vec<_>>()
    );

    let replayed = replay(&trace, Arbiters::Planned);

    assert_eq!(replayed.epochs.len(), trace.epochs.len());
    for (again, recorded) in replayed.epochs.iter().zip(&trace.epochs) {
        assert_eq!(
            again.allocation, recorded.allocation,
            "epoch {:?} decided again",
            recorded.allocation.epoch
        );
        assert_eq!(
            again.why, recorded.why,
            "epoch {:?} explained again",
            recorded.allocation.epoch
        );
    }
    assert_eq!(replayed.metrics.same_as_recorded, trace.epochs.len());
}

/// **A recorded epoch plans the switch the DCC measured.** `oak` ran
/// `Balanced` under epoch 0 and measured its switch to it at 3 ms:
/// - while its layer is kept — epochs 1 to `KEEP_EPOCHS` — every round
///   names a switch back to it at zero: it prepares nothing, and pays only
///   the transient;
/// - from then on, every round names it at 3 ms;
/// - no round charges `oak`'s running floor;
/// - `pine`, which measured nothing, is charged the 5 ms prior for its
///   upgrade.
#[test]
fn a_recorded_epoch_plans_the_switch_the_dcc_measured() {
    let trace = planned_journal();
    let (oak, pine) = (AgentId::new("oak"), AgentId::new("pine"));
    let keep = u64::from(khora_core::agent::KEEP_EPOCHS);
    let priced = |epoch: &EpochRecord, agent: AgentId, row: StrategyId| {
        (epoch.planning.switch.iter())
            .find(|(named, id, _)| *named == agent && *id == row)
            .map(|(_, _, prepare)| *prepare)
    };
    assert!(
        (trace.epochs.iter()).any(|epoch| epoch.allocation.epoch.0 > keep),
        "the DCC recorded epochs past {keep}: {:?}",
        (trace.epochs.iter())
            .map(|e| e.allocation.epoch)
            .collect::<Vec<_>>()
    );

    for epoch in &trace.epochs {
        let number = epoch.allocation.epoch.0;
        let balanced = priced(epoch, oak, Balanced);
        if number <= keep {
            assert_eq!(
                balanced,
                Some(Duration::ZERO),
                "epoch {number}: oak's Balanced is kept, in {:?}",
                epoch.planning
            );
        } else {
            assert_eq!(
                balanced,
                Some(us(3_000)),
                "epoch {number}: oak's Balanced as measured, in {:?}",
                epoch.planning
            );
        }
        assert_eq!(
            priced(epoch, oak, LowPower),
            None,
            "epoch {number}: oak runs its floor"
        );
        assert_eq!(
            priced(epoch, pine, HighPerformance),
            Some(Duration::from_millis(5)),
            "epoch {number}: pine measured nothing, in {:?}",
            epoch.planning
        );
    }
}
