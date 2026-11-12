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

//! A kept layer swapped back in watches its price as a fresh one would: what
//! it saw before it was swapped out says nothing about the envelope it runs
//! under now.

use khora_agents::lane_agent::{lane_agent, LaneAgent};
use khora_core::agent::gorna::protocol::Epoch;
use khora_core::agent::{Agent, Wake};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::OutputDeck;

use super::fixtures::{host_envelope, run, runtime, wakes_of, Plan, Recorded, LADDER};

/// Runs one frame of `agent` on a load of `load`; the ladder level it
/// recorded.
fn level_at(agent: &mut LaneAgent, number: u64, load: u32) -> Option<usize> {
    let mut deck = OutputDeck::new();
    run(agent, number, &runtime(load, Plan::default()), &mut deck);
    deck.get::<Recorded>().map(|recorded| recorded.level)
}

/// **A reinstated layer whose quote stops fitting wakes its agent.** The
/// ladder's middle level costs `2 × load` µs. Lent 205 µs on a load of 100,
/// it runs at 200 µs; on 104 it costs 208 µs, stops fitting, and the agent
/// moves to the first level — the middle layer is kept. Lent 210 µs, the
/// agent reinstates it. On a load of 109 it costs 218 µs: within the drift of
/// its 200 µs quote, beyond the 210 µs envelope. A fresh layer would raise
/// `QuoteUnfit` on that run; the reinstated one must too, whatever it saw
/// before it was kept — else it runs over its envelope until the quote ages.
#[test]
fn a_reinstated_layer_whose_quote_stops_fitting_wakes_its_agent() {
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());
    agent.apply(Epoch(1), &host_envelope(205), None, None);

    assert_eq!(level_at(&mut agent, 1, 100), Some(1), "200 µs fits 205 µs");
    assert_eq!(level_at(&mut agent, 2, 104), Some(1));
    assert!(wakes_of(&agent).contains(&Wake::QuoteUnfit));
    assert_eq!(
        level_at(&mut agent, 3, 104),
        Some(1),
        "woken, the first level staged"
    );
    assert_eq!(
        level_at(&mut agent, 4, 104),
        Some(0),
        "the first level runs"
    );
    assert!(wakes_of(&agent).is_empty(), "{:?}", wakes_of(&agent));

    agent.apply(Epoch(2), &host_envelope(210), None, None);
    assert_eq!(
        level_at(&mut agent, 5, 104),
        Some(0),
        "woken, the middle level reinstated"
    );
    assert_eq!(
        level_at(&mut agent, 6, 109),
        Some(1),
        "the kept middle layer runs"
    );

    assert!(
        wakes_of(&agent).contains(&Wake::QuoteUnfit),
        "the reinstated layer ran at 218 µs under a 210 µs envelope and woke nothing: {:?}",
        wakes_of(&agent)
    );
}

/// **A reinstated layer refused an input wakes its agent.** The ladder's
/// middle level runs; a frame without its plan refuses it (`Missing`), the
/// agent drops the ladder's prices and stages the first level, and the middle
/// layer is kept — refused. The plan back, the first level runs and prices
/// the ladder again; lent 205 µs, the agent reinstates the middle layer. A
/// frame without the plan refuses it again: a fresh layer would raise
/// `InputRefused(Missing)` — the agent must hear of it, and drop the price it
/// chose the row on — whatever the layer was refused before it was kept.
#[test]
fn a_reinstated_layer_refused_an_input_wakes_its_agent() {
    use khora_core::agent::gorna::protocol::OfferRequest;
    use khora_core::lane::guard::Reason;
    use std::sync::Arc;

    let without_a_plan = || {
        let mut runtime = khora_core::Runtime::new();
        runtime.resources.insert(super::fixtures::Load(100));
        Arc::new(runtime)
    };
    let with_a_plan = || runtime(100, Plan::default());
    let mut deck = OutputDeck::new();
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());
    agent.apply(Epoch(1), &host_envelope(205), None, None);

    run(&mut agent, 1, &with_a_plan(), &mut deck);
    run(&mut agent, 2, &without_a_plan(), &mut deck);
    assert!(wakes_of(&agent).contains(&Wake::InputRefused(Reason::Missing)));
    run(&mut agent, 3, &without_a_plan(), &mut deck);
    let first = run(&mut agent, 4, &with_a_plan(), &mut deck).expect("a layer ran");
    assert_eq!(first.row, khora_core::agent::gorna::StrategyId::LowPower);

    let request: OfferRequest = super::fixtures::offer_request(2);
    let _ = agent.offer(&request);
    agent.apply(Epoch(2), &host_envelope(205), None, None);
    run(&mut agent, 5, &with_a_plan(), &mut deck);
    let _ = run(&mut agent, 6, &without_a_plan(), &mut deck);
    assert_eq!(
        super::fixtures::installed_row(&agent),
        Some(khora_core::agent::gorna::StrategyId::Balanced),
        "the kept middle layer was reinstated"
    );

    assert!(
        wakes_of(&agent).contains(&Wake::InputRefused(Reason::Missing)),
        "the reinstated layer was refused its plan and woke nothing: {:?}",
        wakes_of(&agent)
    );
}
