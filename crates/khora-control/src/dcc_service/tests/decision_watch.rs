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

//! What the DCC tells a tool of its last decision: the view it publishes
//! after each live round and each replayed epoch, and the rows planned its
//! journal records.

use super::*;
use khora_core::agent::gorna::protocol::{Binding, DecidedBy, Rejected, Why};
use khora_core::agent::gorna::{Correction, DecisionView, Planning};
use khora_core::lane::cost::Device;

/// A DCC ticking at 100 Hz and recording its epochs, with `renderer`
/// offering a 2 ms floor and a 5 ms `Balanced` — an upgrade a 16 ms frame
/// has room for — and `audio` a 1 ms floor alone; not started.
fn a_recording_dcc_of_two_agents() -> (DccService, Receiver<TelemetryEvent>) {
    let (dcc, rx) = recording(DccConfig::default());
    let (renderer, _) = Speaker::with_upgrade(
        "renderer",
        Duration::from_millis(2),
        Duration::from_millis(5),
    );
    let (audio, _) = Speaker::with_ear("audio", Duration::from_millis(1));
    dcc.register_agent(renderer, 1.0);
    dcc.register_agent(audio, 0.5);
    (dcc, rx)
}

/// `rows`, by agent.
fn sorted(mut rows: Vec<(AgentId, StrategyId)>) -> Vec<(AgentId, StrategyId)> {
    rows.sort_by_key(|(agent, _)| *agent);
    rows
}

/// The host time `record` lends `name`.
fn host_time_lent(record: &EpochRecord, name: &'static str) -> Duration {
    (record.allocation.envelopes.iter())
        .find(|(agent, _)| *agent == AgentId::new(name))
        .map(|(_, envelope)| envelope.time.cpu)
        .unwrap_or_else(|| panic!("no envelope for `{name}`: {:?}", record.allocation))
}

/// **The DCC publishes its last decision.** After live rounds, the DCC's
/// decision watch holds the last one, as the journal recorded it: its epoch,
/// the trigger that opened it, the frame's target, why it was decided, each
/// agent's envelope as lent, the corrections and the planning it decided on
/// — and the rows it planned, `renderer`'s upgrade and `audio`'s floor. It
/// was decided live, not replayed.
#[test]
fn the_dcc_publishes_its_last_decision() {
    let (mut dcc, rx) = a_recording_dcc_of_two_agents();
    dcc.start(rx);

    let decided = wait_for(3, || !records(&dcc).is_empty());
    dcc.stop();
    assert!(decided, "the DCC opened an epoch");
    let last = records(&dcc).last().cloned().expect("the last epoch");
    assert!(
        host_time_lent(&last, "renderer") >= Duration::from_millis(5),
        "the round has room for the renderer's 5 ms upgrade: {:?}",
        last.allocation
    );

    let view = dcc
        .decision_watch()
        .latest()
        .expect("the DCC published its decision");

    assert_eq!(view.epoch, last.allocation.epoch);
    assert_eq!(view.trigger, last.allocation.trigger);
    assert_eq!(view.target, last.target);
    assert_eq!(view.why, last.why);
    assert_eq!(view.envelopes, last.allocation.envelopes);
    assert_eq!(view.corrections, last.corrections);
    assert_eq!(view.planning, last.planning);
    assert_eq!(
        sorted(view.planned.clone()),
        vec![
            (AgentId::new("audio"), StrategyId::LowPower),
            (AgentId::new("renderer"), StrategyId::Balanced),
        ],
        "the rows planned: {view:?}"
    );
    assert!(!view.replayed, "decided live");
}

/// The epoch a journal recorded as `epoch`: `renderer` lent `renderer_us` on
/// the host and planned at `Balanced`, decided by the exact search, bound by
/// the graphics processor's time with the renderer's `HighPerformance`
/// refused for it, under a 33 ms target, the renderer's `Balanced` learnt at
/// 1.5 ± 0.25, and planned with a 2 ms margin.
fn a_recorded_epoch(epoch: u64, renderer_us: u64) -> EpochRecord {
    let renderer = AgentId::new("renderer");
    let mut recorded = record(allocation(
        epoch,
        Trigger::Overruns,
        vec![(renderer, cpu_envelope(renderer_us))],
    ));
    recorded.target = Duration::from_micros(33_333);
    recorded.why = Why {
        by: DecidedBy::Exact,
        trigger: Trigger::Overruns,
        binding: Some(Binding::Time(Device::Gpu)),
        rejected: vec![Rejected {
            agent: renderer,
            row: StrategyId::HighPerformance,
            over: Binding::Time(Device::Gpu),
        }],
    };
    recorded.corrections = vec![(
        renderer,
        StrategyId::Balanced,
        Correction {
            factor: 1.5,
            spread: 0.25,
        },
    )];
    recorded.planning = Planning {
        margin: Duration::from_millis(2),
        ..Planning::default()
    };
    recorded.planned = vec![(renderer, StrategyId::Balanced)];
    recorded
}

/// **A replayed epoch publishes the reasons it replays.** A journal of two
/// epochs is replayed, with no agent registered so no live round follows:
/// the DCC's decision watch holds the last epoch replayed — its allocation's
/// epoch, trigger and envelopes, its target, its `why`, its planned rows, its
/// corrections and its planning, as recorded — and says it was replayed.
#[test]
fn a_replayed_epoch_publishes_the_reasons_it_replays() {
    let (mut dcc, rx) = DccService::new(DccConfig {
        tick_rate: 100,
        ..Default::default()
    });
    let first = a_recorded_epoch(41, 3_000);
    let last = a_recorded_epoch(42, 4_500);
    dcc.replay_decisions(DecisionTrace {
        epochs: vec![first, last.clone()],
    });
    dcc.start(rx);

    let replayed = wait_for(3, || !dcc.is_replaying());
    // A few ticks more, for a DCC that would publish late.
    thread::sleep(Duration::from_millis(50));
    dcc.stop();
    assert!(replayed, "the journal was replayed to its end");

    let view = dcc.decision_watch().latest();

    assert_eq!(
        view,
        Some(DecisionView {
            epoch: last.allocation.epoch,
            trigger: last.allocation.trigger,
            target: last.target,
            why: last.why.clone(),
            envelopes: last.allocation.envelopes.clone(),
            planned: last.planned.clone(),
            corrections: last.corrections.clone(),
            planning: last.planning.clone(),
            replayed: true,
        })
    );
}

/// **The journal records the rows planned.** Recording, every epoch the DCC
/// journals names the rows its round planned: `renderer`'s 5 ms upgrade,
/// which the frame has room for, and `audio`'s floor. The last one is the
/// rows the published decision names.
#[test]
fn the_journal_records_the_rows_planned() {
    let (mut dcc, rx) = a_recording_dcc_of_two_agents();
    dcc.start(rx);

    let decided = wait_for(3, || !records(&dcc).is_empty());
    dcc.stop();
    assert!(decided, "the DCC opened an epoch");
    let journaled = records(&dcc);

    for record in &journaled {
        assert!(
            host_time_lent(record, "renderer") >= Duration::from_millis(5),
            "the round has room for the renderer's 5 ms upgrade: {:?}",
            record.allocation
        );
        assert_eq!(
            sorted(record.planned.clone()),
            vec![
                (AgentId::new("audio"), StrategyId::LowPower),
                (AgentId::new("renderer"), StrategyId::Balanced),
            ],
            "epoch {:?} names the rows it planned",
            record.allocation.epoch
        );
    }
    let last = journaled.last().expect("the last epoch");
    let view = dcc
        .decision_watch()
        .latest()
        .expect("the DCC published its decision");
    assert_eq!(sorted(last.planned.clone()), sorted(view.planned));
}

/// **A live round after a replay publishes its own decision.** A journal of
/// one epoch, written before the rows planned were kept, is replayed to a
/// recording DCC of `renderer` and `audio`; once it runs out, a live round
/// opens. The decision watch then holds that live round, as the journal
/// recorded it — its epoch, past the replayed one, its target, its `why` and
/// the rows it planned — and says it was decided live, not replayed.
#[test]
fn a_live_round_after_a_replay_publishes_its_own_decision() {
    let (mut dcc, rx) = a_recording_dcc_of_two_agents();
    let mut older = a_recorded_epoch(41, 3_000);
    older.planned = Vec::new();
    dcc.replay_decisions(DecisionTrace {
        epochs: vec![older],
    });
    dcc.start(rx);

    let live = wait_for(3, || {
        records(&dcc)
            .iter()
            .any(|record| record.allocation.epoch.0 > 41)
    });
    // A few ticks more, for a DCC that would publish late.
    thread::sleep(Duration::from_millis(50));
    dcc.stop();
    assert!(live, "a live round followed the replay");
    let last = records(&dcc).last().cloned().expect("the live round");
    let view = dcc
        .decision_watch()
        .latest()
        .expect("the DCC published its decision");

    assert!(!view.replayed, "decided live: {view:?}");
    assert_eq!(view.epoch, last.allocation.epoch);
    assert!(view.epoch.0 > 41, "{view:?}");
    assert_eq!(view.target, last.target);
    assert_eq!(view.why, last.why);
    assert_eq!(view.envelopes, last.allocation.envelopes);
    assert_eq!(sorted(view.planned.clone()), sorted(last.planned.clone()));
    assert_eq!(
        sorted(view.planned),
        vec![
            (AgentId::new("audio"), StrategyId::LowPower),
            (AgentId::new("renderer"), StrategyId::Balanced),
        ]
    );
}
