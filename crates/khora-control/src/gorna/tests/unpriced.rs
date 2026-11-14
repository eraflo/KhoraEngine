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

//! An agent with no priced row: the allocation names no envelope for it, so it
//! keeps what it holds or runs at its own price, while a priced or late agent
//! is lent as before.

use super::decide::{bid, round};
use super::*;
use khora_core::agent::gorna::protocol::Trigger;

use AgentImportance::Important;
use StrategyId::{HighPerformance, LowPower};

/// A row of `id` with no price: no time, nothing held, its cost a guess.
fn assumed(id: StrategyId, quality: f32) -> OfferRow {
    OfferRow {
        id,
        cost: Cost::default(),
        quality,
        confidence: CostBasis::Assumed,
    }
}

/// `name` offering its `LowPower` floor alone, unpriced: what a family that
/// has quoted nothing yet offers.
fn unpriced_offer(name: &'static str) -> Offer {
    offer(name, vec![assumed(LowPower, 0.4)], LowPower)
}

/// `ash`: its floor `LowPower`, 2 ms at 0.4, and `HighPerformance`, 3 ms at
/// 1.0, both priced.
fn priced_offer() -> Offer {
    offer(
        "ash",
        vec![row(LowPower, 2_000, 0.4), row(HighPerformance, 3_000, 1.0)],
        LowPower,
    )
}

/// Whether `allocation` names an envelope for `name`.
fn names(allocation: &Allocation, name: &'static str) -> bool {
    allocation
        .envelopes
        .iter()
        .any(|(agent, _)| *agent == AgentId::new(name))
}

/// **An agent with no priced row is named in no envelope.**
///
/// In a 16 ms frame, `ash` offers priced rows and `script` its floor alone,
/// unpriced. The allocation names no envelope for `script`, and lends `ash` the
/// envelope it would be lent were `script` not there.
#[test]
fn an_agent_with_no_priced_row_is_named_in_no_envelope() {
    let alone = [bid(priced_offer(), Important)];
    let together = [
        bid(priced_offer(), Important),
        bid(unpriced_offer("script"), Important),
    ];

    let without = decide(&round(Trigger::Refresh, 16_000, &alone));
    let with = decide(&round(Trigger::Refresh, 16_000, &together));

    assert!(
        !names(&with, "script"),
        "an agent with no priced row is lent nothing: {with:?}"
    );
    assert_eq!(
        lent_to(&with, "ash"),
        lent_to(&without, "ash"),
        "the priced agent is named as before"
    );
}

/// **The arbitrator names no envelope for an agent with no priced row.**
///
/// `ash` answers priced rows and `script` an unpriced floor in a live round of
/// a 16 ms frame: the allocation names `ash`, with room for its
/// `HighPerformance`, and not `script`.
#[test]
fn the_arbitrator_names_no_envelope_for_an_agent_with_no_priced_row() {
    let (_, ash) = shared(Bidder::new(
        "ash",
        vec![row(LowPower, 2_000, 0.4), row(HighPerformance, 3_000, 1.0)],
        LowPower,
    ));
    let (_, script) = shared(Bidder::new(
        "script",
        vec![assumed(LowPower, 0.4)],
        LowPower,
    ));
    let mut arbitrator = arbitrator();

    let allocation = arbitrator.allocate(
        &playing(),
        &frame_of(16.0),
        Trigger::Initial,
        &[ash, script],
        &nothing(),
    );

    assert!(
        !names(&allocation, "script"),
        "an agent with no priced row is lent nothing: {allocation:?}"
    );
    assert!(
        fits(&cpu_cost(3_000), &lent_to(&allocation, "ash")),
        "the priced agent is lent its upgrade: {allocation:?}"
    );
}

/// **A `Safety` round names no envelope for an agent with no priced row.**
///
/// In a 16 ms frame opened as `Safety`, `ash` offers priced rows and `script`
/// its floor alone, unpriced. Every agent with a price is lent its floor;
/// `script`, whose floor is a guess of no time, is named in no envelope — a
/// zero-time one would starve it as surely as in any other round.
#[test]
fn a_safety_round_names_no_envelope_for_an_agent_with_no_priced_row() {
    let bids = [
        bid(priced_offer(), Important),
        bid(unpriced_offer("script"), Important),
    ];

    let allocation = decide(&round(Trigger::Safety, 16_000, &bids));

    assert_eq!(allocation.trigger, Trigger::Safety);
    assert!(
        !names(&allocation, "script"),
        "an agent with no priced row is lent nothing, a Safety round included: {allocation:?}"
    );
}

/// **A pin on an agent with no priced row still reaches it.**
///
/// `script` offers two unpriced rows and a developer pins it to
/// `HighPerformance` (`Manual`). The allocation names the pin; lent at a frame
/// start, it reaches the agent — a developer's pin is no envelope, and the
/// round needs no price to deliver it.
#[test]
fn a_pin_on_an_agent_with_no_priced_row_still_reaches_it() {
    let mut pinned = bid(
        offer(
            "script",
            vec![assumed(LowPower, 0.4), assumed(HighPerformance, 1.0)],
            LowPower,
        ),
        Important,
    );
    pinned.mode = AdaptationMode::Manual(HighPerformance);
    let bids = [bid(priced_offer(), Important), pinned];

    let allocation = decide(&round(Trigger::Refresh, 16_000, &bids));
    let mut script = Bidder::new(
        "script",
        vec![assumed(LowPower, 0.4), assumed(HighPerformance, 1.0)],
        LowPower,
    );
    let reached = allocation.lend_to(&mut script);

    assert!(
        reached,
        "the allocation names the pin {:?} but lends the agent nothing: {allocation:?}",
        allocation.pins
    );
    assert_eq!(script.last_lent().pin, Some(HighPerformance));
}

/// **A late agent with no priced row keeps the envelope it holds.**
///
/// `script` answers late, its floor alone unpriced, holding 3 ms from an
/// earlier round; `elm`, on time, offers the same unpriced floor and holds
/// nothing. The allocation names `script` with the envelope it holds, and names
/// no envelope for `elm`.
#[test]
fn a_late_agent_with_no_priced_row_keeps_the_envelope_it_holds() {
    let mut late = bid(unpriced_offer("script"), Important);
    late.late = true;
    late.kept = Some(cpu_envelope(3_000));
    let bids = [
        bid(priced_offer(), Important),
        late,
        bid(unpriced_offer("elm"), Important),
    ];

    let allocation = decide(&round(Trigger::Refresh, 16_000, &bids));

    assert_eq!(
        lent_to(&allocation, "script"),
        cpu_envelope(3_000),
        "the late agent keeps what it holds: {allocation:?}"
    );
    assert!(
        !names(&allocation, "elm"),
        "an agent on time with no priced row is lent nothing: {allocation:?}"
    );
}

/// **A late agent with no priced row is lent no zero-time floor in a `Safety`
/// round.**
///
/// In a 16 ms frame opened as `Safety`, `script` answers late, its floor alone
/// unpriced, holding 3 ms from an earlier round. Its floor is a guess of no
/// time: lent as an envelope it would starve the agent. The allocation either
/// names no envelope for it or lends it the 3 ms it holds — never an envelope
/// of no time.
#[test]
fn a_late_agent_with_no_priced_row_is_lent_no_zero_time_floor_in_a_safety_round() {
    let mut late = bid(unpriced_offer("script"), Important);
    late.late = true;
    late.kept = Some(cpu_envelope(3_000));
    let bids = [bid(priced_offer(), Important), late];

    let allocation = decide(&round(Trigger::Safety, 16_000, &bids));

    assert_eq!(allocation.trigger, Trigger::Safety);
    let lent = (allocation.envelopes.iter())
        .find(|(agent, _)| *agent == AgentId::new("script"))
        .map(|(_, envelope)| *envelope);
    assert!(
        lent.is_none_or(|envelope| envelope == cpu_envelope(3_000)),
        "the late agent with no priced row is lent {lent:?}, a floor of no time: {allocation:?}"
    );
}
