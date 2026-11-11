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

//! What the arbitrator holds of its last decision, to explain it: the rows a
//! live round planned, and, once a journal's epoch is reissued, that epoch's
//! own reasons rather than the last live round's.

use super::*;
use khora_core::agent::gorna::protocol::{Binding, DecidedBy, Rejected, Trigger};
use khora_core::lane::cost::Device;

use StrategyId::{Balanced, HighPerformance, LowPower};

const ASH: AgentId = AgentId::new("ash");
const ELM: AgentId = AgentId::new("elm");

/// `ash`: its floor `LowPower`, 1 ms at 0.4, and `HighPerformance`, 3 ms at
/// 1.0 — an upgrade a 16 ms frame has room for.
fn ash() -> Bidder {
    Bidder::new(
        "ash",
        vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 3_000, 1.0)],
        LowPower,
    )
}

/// `elm`: its floor `LowPower`, 2 ms at 0.4, and `Balanced`, 20 ms at 0.9 —
/// an upgrade longer than a 16 ms frame alone.
fn elm() -> Bidder {
    Bidder::new(
        "elm",
        vec![row(LowPower, 2_000, 0.4), row(Balanced, 20_000, 0.9)],
        LowPower,
    )
}

/// The best row of `rows` that `envelope` fits, as an agent chooses on its
/// own quotes: its floor, the first row, when none fits.
fn best_fitting(rows: &[OfferRow], envelope: &Envelope) -> StrategyId {
    rows.iter()
        .filter(|row| fits(&row.cost, envelope))
        .max_by(|a, b| a.quality.total_cmp(&b.quality))
        .map_or(rows[0].id, |row| row.id)
}

/// One live round of `ash` and `elm` in a 16 ms frame, opened by `Initial`.
fn a_live_round(arbitrator: &mut GornaArbitrator) -> Allocation {
    let (_, ash) = shared(ash());
    let (_, elm) = shared(elm());
    arbitrator.allocate(
        &playing(),
        &frame_of(16.0),
        Trigger::Initial,
        &[ash, elm],
        &nothing(),
    )
}

/// **The arbitrator keeps the rows it planned.** In a 16 ms frame, `ash`'s
/// 3 ms upgrade fits and `elm`'s 20 ms one does not: the round plans `ash`'s
/// `HighPerformance` and `elm`'s floor, and lends each an envelope that fits
/// the row planned for it, in its quotes. `planned()` names each agent once,
/// with that row — the row the agent runs on what it was lent.
#[test]
fn the_arbitrator_keeps_the_rows_it_planned() {
    let mut arbitrator = arbitrator();

    let allocation = a_live_round(&mut arbitrator);

    // What each agent runs on what it is lent, by its own quotes.
    let runs_ash = best_fitting(&ash().rows, &lent_to(&allocation, "ash"));
    let runs_elm = best_fitting(&elm().rows, &lent_to(&allocation, "elm"));
    assert_eq!(runs_ash, HighPerformance, "{allocation:?}");
    assert_eq!(runs_elm, LowPower, "{allocation:?}");

    let mut planned = arbitrator.planned().to_vec();
    planned.sort_by_key(|(agent, _)| *agent);
    assert_eq!(
        planned,
        vec![(ASH, runs_ash), (ELM, runs_elm)],
        "each agent's planned row, the one its envelope fits"
    );
}

/// The reasons a journal recorded for epoch 99, each unlike what a live round
/// of `ash` and `elm` in a 16 ms frame decides: decided by the exact search,
/// bound by the graphics processor's time, `elm`'s `Balanced` refused for
/// it; a 33 ms target; a 2 ms margin and a 4 ms switch of `ash` to
/// `Balanced`; `ash`'s `Balanced` learnt at 1.5 ± 0.25; `ash` planned at
/// `Balanced` and `elm` at its floor; `ash` lent 7 ms and `elm` 2.5 ms.
fn a_recorded_epoch() -> EpochRecord {
    let lent = |micros| Envelope {
        time: PerDevice {
            cpu: us(micros),
            ..PerDevice::default()
        },
        ..Envelope::default()
    };
    EpochRecord {
        allocation: Allocation {
            epoch: Epoch(99),
            trigger: Trigger::Overruns,
            envelopes: vec![(ASH, lent(7_000)), (ELM, lent(2_500))],
            pins: Vec::new(),
            allowed: Vec::new(),
            price: Some(0.125),
            lever: true,
            late: Vec::new(),
        },
        target: us(33_333),
        offers: Vec::new(),
        reports: Vec::new(),
        waves: Vec::new(),
        policies: Vec::new(),
        hints: Vec::new(),
        reasons: vec!["the frame ran over its target".to_owned()],
        why: Why {
            by: DecidedBy::Exact,
            trigger: Trigger::Overruns,
            binding: Some(Binding::Time(Device::Gpu)),
            rejected: vec![Rejected {
                agent: ELM,
                row: Balanced,
                over: Binding::Time(Device::Gpu),
            }],
        },
        importance: Vec::new(),
        corrections: vec![(
            ASH,
            Balanced,
            Correction {
                factor: 1.5,
                spread: 0.25,
            },
        )],
        memory: PerDevice::default(),
        background: Vec::new(),
        at: Duration::from_secs(3),
        frames: khora_core::agent::gorna::FrameSummary::default(),
        in_force: Vec::new(),
        planning: Planning {
            horizon: Duration::from_secs(2),
            transient: ms(100),
            margin: ms(2),
            growth: vec![(ASH, 1.5)],
            switch: vec![(ASH, Balanced, ms(4))],
            caution: 0.5,
        },
        planned: vec![(ASH, Balanced), (ELM, LowPower)],
    }
}

/// **A reissued record puts its reasons in force.** After a live round, a
/// journal's epoch is reissued: the allocation in force is the record's, and
/// so are the reasons the arbitrator gives for it — its `why`, its planning,
/// its corrections, its planned rows and its target — not the live round's.
/// A replayed allocation is never explained by another round's reasons.
#[test]
fn a_reissued_record_puts_its_reasons_in_force() {
    let mut arbitrator = arbitrator();
    a_live_round(&mut arbitrator);
    let record = a_recorded_epoch();
    assert_ne!(arbitrator.why(), &record.why, "the live round's own why");
    assert_ne!(
        arbitrator.target(),
        record.target,
        "the live round's target"
    );

    arbitrator.reissue(&record);

    assert_eq!(arbitrator.in_force(), Some(&record.allocation));
    assert_eq!(arbitrator.why(), &record.why);
    assert_eq!(arbitrator.planning(), &record.planning);
    assert_eq!(arbitrator.corrections(), record.corrections.as_slice());
    assert_eq!(arbitrator.planned(), record.planned.as_slice());
    assert_eq!(arbitrator.target(), record.target);
}

/// Whether every row `planned` names is the row its agent runs on what
/// `allocation` lends it, by its own quotes `rows_of`.
fn each_planned_row_is_run(
    planned: &[(AgentId, StrategyId)],
    allocation: &Allocation,
    rows_of: &[(AgentId, Vec<OfferRow>)],
) -> Vec<(AgentId, StrategyId, StrategyId)> {
    planned
        .iter()
        .filter_map(|(agent, row)| {
            let rows = &rows_of.iter().find(|(named, _)| named == agent)?.1;
            let lent = allocation
                .envelopes
                .iter()
                .find(|(named, _)| named == agent)
                .map(|(_, envelope)| *envelope)?;
            let runs = best_fitting(rows, &lent);
            (runs != *row).then_some((*agent, *row, runs))
        })
        .collect()
}

/// **A `Safety` round plans the floors it lends.** `ash` is pinned to its
/// `HighPerformance`; the round opens as `Safety`, which lends every agent
/// its floor and no pin. The rows the arbitrator says it planned are the rows
/// the agents run on what they are lent: `ash`'s floor, not the pin the
/// round did not lend.
#[test]
fn a_safety_round_plans_the_floors_it_lends() {
    let mut arbitrator = arbitrator();
    arbitrator.set_adaptation_mode(ASH, AdaptationMode::Manual(HighPerformance));
    let (_, agent) = shared(ash());

    let allocation = arbitrator.allocate(
        &playing(),
        &frame_of(16.0),
        Trigger::Safety,
        &[agent],
        &nothing(),
    );

    assert_eq!(allocation.trigger, Trigger::Safety);
    assert!(allocation.pins.is_empty(), "{allocation:?}");
    assert_eq!(
        best_fitting(&ash().rows, &lent_to(&allocation, "ash")),
        LowPower,
        "the floor is lent: {allocation:?}"
    );
    let wrong = each_planned_row_is_run(arbitrator.planned(), &allocation, &[(ASH, ash().rows)]);
    assert!(
        wrong.is_empty(),
        "(agent, planned, runs) where the planned row is not the one run: {wrong:?}"
    );
    let view = arbitrator.view(false).expect("a round ran");
    assert_eq!(view.planned, arbitrator.planned());
}

/// **A `Stable` agent that keeps its envelope is planned the row it runs.**
/// A first round lends `ash` its 3 ms `HighPerformance`; `Stable` from then
/// on, the next round keeps its envelope. The row the arbitrator says it
/// planned for `ash`, if it names one, is the row `ash` runs on the envelope
/// kept — not its floor.
#[test]
fn a_stable_agent_keeping_its_envelope_is_planned_the_row_it_runs() {
    let mut arbitrator = arbitrator();
    let (_, agent) = shared(ash());
    let agents = [agent];
    let first = arbitrator.allocate(
        &playing(),
        &frame_of(16.0),
        Trigger::Initial,
        &agents,
        &nothing(),
    );
    assert_eq!(
        best_fitting(&ash().rows, &lent_to(&first, "ash")),
        HighPerformance,
        "{first:?}"
    );

    arbitrator.set_adaptation_mode(ASH, AdaptationMode::Stable);
    let kept = arbitrator.allocate(
        &playing(),
        &frame_of(16.0),
        Trigger::Pressure,
        &agents,
        &nothing(),
    );

    assert_eq!(
        best_fitting(&ash().rows, &lent_to(&kept, "ash")),
        HighPerformance,
        "the envelope kept: {kept:?}"
    );
    let wrong = each_planned_row_is_run(arbitrator.planned(), &kept, &[(ASH, ash().rows)]);
    assert!(
        wrong.is_empty(),
        "(agent, planned, runs) where the planned row is not the one run: {wrong:?}"
    );
}

/// **An agent that answers late is planned the row it runs.** A first round
/// lends `ash` its 3 ms `HighPerformance`; in the next, `ash` answers after
/// the round's deadline and keeps its envelope. The row the arbitrator says it
/// planned for `ash`, if it names one, is the row `ash` runs on the envelope
/// kept — not its floor.
#[test]
fn an_agent_answering_late_is_planned_the_row_it_runs() {
    let mut arbitrator = GornaArbitrator::new(Duration::from_millis(20));
    let (typed, agent) = shared(ash());
    let agents = [agent];
    let first = arbitrator.allocate(
        &playing(),
        &frame_of(16.0),
        Trigger::Initial,
        &agents,
        &nothing(),
    );
    assert_eq!(
        best_fitting(&ash().rows, &lent_to(&first, "ash")),
        HighPerformance,
        "{first:?}"
    );

    typed.lock().expect("the agent lock").late = true;
    let kept = arbitrator.allocate(
        &playing(),
        &frame_of(16.0),
        Trigger::Pressure,
        &agents,
        &nothing(),
    );

    assert_eq!(kept.late, vec![ASH], "{kept:?}");
    assert_eq!(
        best_fitting(&ash().rows, &lent_to(&kept, "ash")),
        HighPerformance,
        "the envelope kept: {kept:?}"
    );
    let wrong = each_planned_row_is_run(arbitrator.planned(), &kept, &[(ASH, ash().rows)]);
    assert!(
        wrong.is_empty(),
        "(agent, planned, runs) where the planned row is not the one run: {wrong:?}"
    );
}
