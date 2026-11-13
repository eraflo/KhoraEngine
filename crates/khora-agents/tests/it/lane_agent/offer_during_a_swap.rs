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

//! What a `LaneAgent` offers while a new layer waits beside the installed
//! one for the next frame start: the row running now is the installed one,
//! the one its last report measured.

use khora_agents::lane_agent::lane_agent;
use khora_core::agent::gorna::protocol::{CostBasis, Epoch};
use khora_core::agent::gorna::StrategyId;
use khora_core::agent::Agent;
use khora_core::gpu::DeviceCaps;
use khora_core::lane::OutputDeck;

use super::fixtures::{
    host_envelope, installed_row, locked, offer_request, place_of, run, runtime, Plan, LADDER,
};

/// **While a layer is staged, the offer's current row is the one running.**
/// The ladder runs its middle level (200 µs under 205 µs); on a load of 104
/// it stops fitting, and the next event pass stages the first level beside
/// it. Until the next frame start the middle layer is the one installed, the
/// one that ran, the one the DCC's measurement is of: an offer made then must
/// name it as `current` — the DCC scales every quote by the measured time
/// over `current`'s quote — and the report must name the row that ran.
#[test]
fn while_a_layer_is_staged_the_offer_names_the_running_row() {
    let mut agent = lane_agent(&LADDER, &DeviceCaps::default());
    agent.apply(Epoch(1), &host_envelope(205), None, None);
    let mut deck = OutputDeck::new();
    run(&mut agent, 1, &runtime(100, Plan::default()), &mut deck);
    run(&mut agent, 2, &runtime(104, Plan::default()), &mut deck);
    let report = run(&mut agent, 3, &runtime(104, Plan::default()), &mut deck)
        .expect("the middle layer ran");
    assert_eq!(report.row, StrategyId::Balanced);
    assert_eq!(installed_row(&agent), Some(StrategyId::Balanced));
    assert_eq!(
        locked(&place_of(&agent)).staged_row(),
        Some(StrategyId::LowPower),
        "the first level waits for the next frame start"
    );

    let offer = agent.offer(&offer_request(2));

    let measured: Vec<StrategyId> = (offer.rows.iter())
        .filter(|row| row.confidence == CostBasis::Measured)
        .map(|row| row.id)
        .collect();
    assert_eq!(measured, vec![StrategyId::Balanced], "the row that ran");
    assert_eq!(
        offer.current,
        StrategyId::Balanced,
        "the offer names the staged row as running, though the installed one ran and runs \
         until the next frame start"
    );
    assert_eq!(
        agent.report().row,
        StrategyId::Balanced,
        "the report names the row that ran"
    );
}
