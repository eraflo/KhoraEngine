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

//! The choice between the rows of equal impact, when they belong to several
//! lanes. A lane's levels are more work for a better result, so within a lane
//! the dearest level that fits is taken; two lanes of equal impact draw the
//! same result, so of the lanes' dearest fitting levels the cheapest is taken.
//! The least impact still comes first, and a full tie keeps the first row.

use std::time::Duration;

use khora_agents::lane_agent::{choose, Choice};
use khora_core::agent::gorna::protocol::Envelope;
use khora_core::agent::gorna::StrategyId;
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::level::{Impact, Level};

use super::fixtures::host_envelope;

/// A level named `id`, of `impact`, holding nothing.
const fn row(id: StrategyId, impact: Impact) -> Level {
    Level {
        id,
        name: "row",
        impact,
        memory: PerDevice {
            cpu: 0,
            gpu: 0,
            npu: 0,
            other: 0,
        },
    }
}

/// A level without impact, named `id`.
const fn plain(id: StrategyId) -> Level {
    row(id, Impact::None)
}

/// A host-only quote of `micros`.
fn host(micros: u64) -> Option<Cost> {
    Some(Cost {
        time: PerDevice {
            cpu: Duration::from_micros(micros),
            ..PerDevice::default()
        },
        ..Cost::default()
    })
}

/// The row [`choose`] picks with no pin and every row allowed, starting on
/// the first row.
fn chosen(envelope: &Envelope, quotes: &[Option<Cost>], menu: &[Level], lanes: &[usize]) -> Choice {
    choose(envelope, quotes, menu, lanes, 0, None, None)
}

/// A choice of `row` within the envelope.
fn within(row: usize) -> Choice {
    Choice {
        row,
        over_budget: false,
    }
}

/// **Within a lane, the dearest level that fits.** One lane of three levels
/// without impact, at 100, 200 and 300 µs: within 1 ms its top level runs,
/// within 250 µs the middle one — not the cheapest, which draws less.
#[test]
fn within_a_lane_of_equal_impact_levels_the_dearest_that_fits_runs() {
    let menu = [
        plain(StrategyId::LowPower),
        plain(StrategyId::Balanced),
        plain(StrategyId::HighPerformance),
    ];
    let quotes = [host(100), host(200), host(300)];
    let lanes = [0, 0, 0];

    assert_eq!(
        chosen(&host_envelope(1_000), &quotes, &menu, &lanes),
        within(2),
        "every level fits: the lane's dearest"
    );
    assert_eq!(
        chosen(&host_envelope(250), &quotes, &menu, &lanes),
        within(1),
        "300 µs no longer fits: the dearest level that does"
    );
}

/// **Across two lanes of equal impact, the cheaper lane.** Two one-level
/// lanes without impact draw the same result; both fit 1 ms, so the cheaper
/// runs — whichever of the two the family lists first.
#[test]
fn of_two_lanes_of_equal_impact_the_cheaper_runs() {
    let menu = [
        plain(StrategyId::Balanced),
        plain(StrategyId::HighPerformance),
    ];
    let lanes = [0, 1];

    assert_eq!(
        chosen(
            &host_envelope(1_000),
            &[host(300), host(200)],
            &menu,
            &lanes
        ),
        within(1),
        "the dearer lane listed first: the cheaper, second, runs"
    );
    assert_eq!(
        chosen(
            &host_envelope(1_000),
            &[host(200), host(300)],
            &menu,
            &lanes
        ),
        within(0),
        "the cheaper lane listed first runs"
    );
}

/// **A lane is weighed by its dearest level that fits.** Lane A has two
/// levels, at 100 and 300 µs; lane B one, at 200 µs; all without impact.
/// Within 1 ms lane A's dearest level, 300 µs, is set against lane B's
/// 200 µs: lane B runs — not A's top level (the dearest row) nor A's bottom
/// one (the cheapest row). Within 250 µs, A's dearest level that fits is its
/// 100 µs one, cheaper than B: lane A runs at its bottom level.
#[test]
fn a_lanes_dearest_fitting_level_is_weighed_against_the_other_lanes() {
    let menu = [
        plain(StrategyId::LowPower),
        plain(StrategyId::Balanced),
        plain(StrategyId::HighPerformance),
    ];
    let quotes = [host(100), host(300), host(200)];
    let lanes = [0, 0, 1];

    assert_eq!(
        chosen(&host_envelope(1_000), &quotes, &menu, &lanes),
        within(2),
        "lane B at 200 µs beats lane A's dearest level at 300 µs"
    );
    assert_eq!(
        chosen(&host_envelope(250), &quotes, &menu, &lanes),
        within(0),
        "lane A's dearest level that fits, 100 µs, beats lane B at 200 µs"
    );
}

/// **The least impact still comes first.** Four one-level lanes: a `Minor`
/// one at 50 µs, two without impact at 400 and 300 µs, a `Minor` one at
/// 900 µs. Within 1 ms the cheaper of the two lanes without impact runs —
/// neither the cheaper nor the dearer row of a worse impact.
#[test]
fn the_least_impact_beats_cheaper_and_dearer_rows_of_a_worse_impact() {
    let menu = [
        row(StrategyId::LowPower, Impact::Minor),
        plain(StrategyId::Balanced),
        plain(StrategyId::HighPerformance),
        row(StrategyId::Custom(1), Impact::Minor),
    ];
    let quotes = [host(50), host(400), host(300), host(900)];
    let lanes = [0, 1, 2, 3];

    assert_eq!(
        chosen(&host_envelope(1_000), &quotes, &menu, &lanes),
        within(2),
        "no impact first, then the cheaper of the two lanes without impact"
    );
}

/// **The lanes' levels are weighed after the impact.** Lane A's top level is
/// `Minor`, its bottom one has no impact; lane B has one level without
/// impact. Of lane A only the bottom level competes on the least impact, and
/// it is cheaper than lane B: lane A runs at its bottom level, not at its
/// dearer, `Minor` top.
#[test]
fn a_lanes_level_of_worse_impact_does_not_stand_for_the_lane() {
    let menu = [
        plain(StrategyId::LowPower),
        row(StrategyId::Balanced, Impact::Minor),
        plain(StrategyId::HighPerformance),
    ];
    let quotes = [host(100), host(300), host(200)];
    let lanes = [0, 0, 1];

    assert_eq!(
        chosen(&host_envelope(1_000), &quotes, &menu, &lanes),
        within(0),
        "lane A's level without impact, 100 µs, beats lane B at 200 µs"
    );
}

/// **The cheaper lane is taken among the allowed rows only.** Three one-level
/// lanes without impact at 300, 100 and 200 µs; the 100 µs one is not
/// allowed: the 200 µs lane runs.
#[test]
fn the_cheaper_lane_is_taken_among_the_allowed_rows() {
    let menu = [
        plain(StrategyId::Balanced),
        plain(StrategyId::LowPower),
        plain(StrategyId::HighPerformance),
    ];
    let quotes = [host(300), host(100), host(200)];
    let lanes = [0, 1, 2];
    let allowed = [StrategyId::Balanced, StrategyId::HighPerformance];

    assert_eq!(
        choose(
            &host_envelope(1_000),
            &quotes,
            &menu,
            &lanes,
            0,
            None,
            Some(&allowed),
        ),
        within(2),
        "the cheapest allowed lane"
    );
}

/// **A full tie keeps the first row.** Three one-level lanes of one impact
/// and one price: the first runs. Two lanes whose dearest fitting levels cost
/// the same: the one listed first runs. One lane of two levels of one price:
/// its first level runs.
#[test]
fn a_full_tie_keeps_the_first_row() {
    let three = [
        plain(StrategyId::LowPower),
        plain(StrategyId::Balanced),
        plain(StrategyId::HighPerformance),
    ];

    assert_eq!(
        chosen(
            &host_envelope(1_000),
            &[host(200), host(200), host(200)],
            &three,
            &[0, 1, 2]
        ),
        within(0),
        "three lanes, one price"
    );
    assert_eq!(
        chosen(
            &host_envelope(1_000),
            &[host(200), host(100), host(200)],
            &three,
            &[0, 1, 1]
        ),
        within(0),
        "lane B's dearest level costs what lane A does: lane A, listed first"
    );
    assert_eq!(
        chosen(
            &host_envelope(1_000),
            &[host(200), host(200)],
            &three[..2],
            &[0, 0]
        ),
        within(0),
        "one lane, two levels of one price"
    );
}

/// **Nothing fits: the floor is the cheapest priced row, whatever its lane.**
/// Two lanes, none of whose levels fits 50 µs: the cheapest row runs, over
/// budget — lane B's bottom level, though lane A is listed first.
#[test]
fn when_nothing_fits_the_floor_is_the_cheapest_row_of_any_lane() {
    let menu = [
        plain(StrategyId::LowPower),
        plain(StrategyId::Balanced),
        plain(StrategyId::Custom(1)),
        plain(StrategyId::HighPerformance),
    ];
    let quotes = [host(300), host(400), host(100), host(200)];
    let lanes = [0, 0, 1, 1];

    assert_eq!(
        chosen(&host_envelope(50), &quotes, &menu, &lanes),
        Choice {
            row: 2,
            over_budget: true,
        },
        "the cheapest priced row, flagged over budget"
    );
}
