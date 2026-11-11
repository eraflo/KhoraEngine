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

//! What the DCC learns of this machine, handed in and out: a profile seeds
//! its cost book as its thread starts, and its thread publishes what the
//! book learnt.

use super::*;
use khora_core::agent::gorna::ProfileRow;
use std::collections::BTreeSet;

/// The renderer's floor, learnt by a profile at `factor`, spread 0.2, over
/// fifty runs.
fn learnt_floor(agent: &str, factor: f64) -> ProfileRow {
    ProfileRow {
        agent: agent.to_owned(),
        row: StrategyId::LowPower,
        factor,
        spread: 0.2,
        runs: 50,
    }
}

/// A run of `name`'s floor quoting 1 ms on the host and taking `factor`
/// times that, as the scheduler publishes it.
fn floor_run(name: &'static str, factor: f64) -> TelemetryEvent {
    TelemetryEvent::AgentReport(Report {
        agent: AgentId::new(name),
        epoch: Epoch::default(),
        row: StrategyId::LowPower,
        measured: PerDevice {
            cpu: Duration::from_secs_f64(factor / 1_000.0),
            ..PerDevice::default()
        },
        done: 1.0,
        health: 1.0,
        over_budget: false,
        stalled: false,
        workload: Some(1_000.0),
        switch: None,
        message: None,
    })
}

/// The factor `rows` name for `agent`'s floor, if they name it.
fn floor_factor(rows: &[ProfileRow], agent: &str) -> Option<f64> {
    (rows.iter())
        .find(|row| row.agent == agent && row.row == StrategyId::LowPower)
        .map(|row| row.factor)
}

/// **The DCC seeds its book and publishes what it learns.** A profile learnt
/// the renderer's floor at 3 times its quote; it also names `ghost`, an
/// agent never registered. Handed to the DCC before it starts:
/// - the first round plans the renderer's floor at 3: the book was seeded as
///   the thread started, before any run;
/// - the DCC publishes what its book learnt: the renderer's floor at 3, and
///   nothing of `ghost`;
/// - eighty runs at the quote teach it: what it publishes comes within 0.1
///   of 1, and still does once the DCC has stopped.
#[test]
fn the_dcc_seeds_its_book_and_publishes_what_it_learns() {
    let (mut dcc, rx) = recording(DccConfig::default());
    let (agent, _ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    dcc.register_agent(agent, 1.0);
    dcc.seed_cost_book(vec![
        learnt_floor("renderer", 3.0),
        learnt_floor("ghost", 9.0),
    ]);
    let tx = dcc.event_sender();
    dcc.start(rx);

    let decided = wait_for(3, || !records(&dcc).is_empty());
    let first = records(&dcc).first().cloned();
    let published = wait_for(3, || {
        floor_factor(&dcc.learnt_costs(), "renderer").is_some_and(|f| (f - 3.0).abs() < 1e-9)
    });
    let seeded = dcc.learnt_costs();

    for _ in 0..80 {
        tx.send(floor_run("renderer", 1.0))
            .expect("the DCC listens");
    }
    // A family switched off — none of the renderer's — opens an epoch, for a
    // DCC that publishes once an epoch.
    let off: BTreeSet<AgentId> = [AgentId::new("elsewhere")].into_iter().collect();
    tx.send(TelemetryEvent::ActivationChanged(off))
        .expect("the DCC listens");
    let taught = wait_for(5, || {
        floor_factor(&dcc.learnt_costs(), "renderer").is_some_and(|f| (f - 1.0).abs() < 0.1)
    });
    dcc.stop();

    assert!(decided, "the DCC opened an epoch");
    let first = first.expect("the first epoch was recorded");
    let planned = (first.corrections.iter())
        .find(|(id, row, _)| *id == AgentId::new("renderer") && *row == StrategyId::LowPower)
        .map(|(_, _, correction)| *correction);
    assert!(
        planned.is_some_and(|c| (c.factor - 3.0).abs() < 1e-9),
        "the first round plans the floor as the profile learnt it: {:?}",
        first.corrections
    );
    assert!(published, "the seeded row is published: {seeded:?}");
    assert!(
        seeded.iter().all(|row| row.agent != "ghost"),
        "no row of an agent never registered: {seeded:?}"
    );
    assert!(
        taught,
        "eighty runs at the quote are learnt: {:?}",
        dcc.learnt_costs()
    );
    let stopped = floor_factor(&dcc.learnt_costs(), "renderer");
    assert!(
        stopped.is_some_and(|f| (f - 1.0).abs() < 0.1),
        "what was learnt, once stopped: {:?}",
        dcc.learnt_costs()
    );
}
