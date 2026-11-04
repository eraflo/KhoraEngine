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

//! The choice, as the pure function of the envelope, the quotes, the menu,
//! the lane of each row, a pin and an allowed list: among the rows whose quote
//! fits the envelope on every device, the least impact, then the dearest
//! level of each lane, then the cheapest of those lanes; the floor and
//! `over_budget` when none fits; the default level when nothing is priced. A
//! pin overrides it; an allowed list filters the rows.
//!
//! The menus here are one lane's levels, so the choice between lanes plays no
//! part; `choice_across_lanes` covers it.

use std::time::Duration;

use khora_agents::agent::{choose, Choice};
use khora_core::agent::gorna::protocol::Envelope;
use khora_core::agent::gorna::StrategyId;
use khora_core::lane::cost::{Cost, PerDevice};
use khora_core::lane::level::{Impact, Level};

use super::fixtures::{host_envelope, ANY_MEMORY};

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

/// The row [`choose`] picks with no pin and every row allowed, `menu` being
/// the levels of one lane.
fn chosen(envelope: &Envelope, quotes: &[Option<Cost>], menu: &[Level], default: usize) -> Choice {
    choose(envelope, quotes, menu, &one_lane(menu), default, None, None)
}

/// The lane of every row of `menu`, when its rows are the levels of one lane.
fn one_lane(menu: &[Level]) -> Vec<usize> {
    vec![0; menu.len()]
}

/// A choice of `row` within the envelope.
fn within(row: usize) -> Choice {
    Choice {
        row,
        over_budget: false,
    }
}

/// Six levels of one lane, in no order of cost or impact: a Major one, a Minor one, three
/// without impact — two cheap, one dear — and one without impact never
/// priced.
const MENU: &[Level] = &[
    row(StrategyId::LowPower, Impact::Major),
    row(StrategyId::Balanced, Impact::Minor),
    row(StrategyId::Custom(1), Impact::None),
    row(StrategyId::Custom(2), Impact::None),
    row(StrategyId::HighPerformance, Impact::None),
    row(StrategyId::Custom(3), Impact::None),
];

/// [`MENU`]'s lanes: its rows are one lane's levels.
const MENU_LANES: &[usize] = &[0; 6];

/// [`MENU`]'s quotes: 100, 300, 150, 200 and 900 µs, the last row unpriced.
fn menu_quotes() -> Vec<Option<Cost>> {
    vec![host(100), host(300), host(150), host(200), host(900), None]
}

/// **The least impact that fits, then the dearest.** Within 400 µs four rows
/// fit; two of them have no impact, and of those the dearer, 200 µs, is taken
/// — over the dearer Minor row at 300 µs, and over the unpriced row, which
/// fits nothing since it has no quote. Within 180 µs the cheap row without
/// impact is the only one left of its kind; within 120 µs only the Major row
/// fits.
#[test]
fn the_agent_takes_the_least_impact_row_that_fits_then_the_dearest() {
    let quotes = menu_quotes();

    assert_eq!(
        chosen(&host_envelope(400), &quotes, MENU, 0),
        within(3),
        "no impact beats a dearer Minor row, then the dearest of its kind"
    );
    assert_eq!(
        chosen(&host_envelope(180), &quotes, MENU, 0),
        within(2),
        "200 µs no longer fits: the 150 µs row without impact"
    );
    assert_eq!(
        chosen(&host_envelope(120), &quotes, MENU, 0),
        within(0),
        "only the Major row fits"
    );
}

/// A quote equal to the envelope fits it.
#[test]
fn a_quote_equal_to_the_envelope_fits() {
    assert_eq!(
        chosen(&host_envelope(200), &menu_quotes(), MENU, 0),
        within(3),
        "200 µs fits an envelope of 200 µs"
    );
}

/// **A row fits only when it fits on every device.** Two rows without
/// impact take the same host time; the dearer one also takes 500 µs of the
/// graphics device, which lends 300: the cheaper one is taken.
#[test]
fn a_row_fits_only_when_it_fits_on_every_device() {
    let menu = [
        row(StrategyId::LowPower, Impact::None),
        row(StrategyId::HighPerformance, Impact::None),
    ];
    let both = |gpu: u64| {
        Some(Cost {
            time: PerDevice {
                cpu: Duration::from_micros(100),
                gpu: Duration::from_micros(gpu),
                ..PerDevice::default()
            },
            ..Cost::default()
        })
    };
    let envelope = Envelope {
        time: PerDevice {
            cpu: Duration::from_micros(400),
            gpu: Duration::from_micros(300),
            ..PerDevice::default()
        },
        memory: ANY_MEMORY,
    };

    assert_eq!(
        chosen(&envelope, &[both(0), both(500)], &menu, 0),
        within(0),
        "the dearer row takes more of the graphics device than the envelope lends"
    );
}

/// **The envelope's memory counts too.** The dearer row holds 2000 bytes on
/// the graphics device, which lends 1000: the cheaper one is taken.
#[test]
fn a_row_holding_more_memory_than_the_envelope_does_not_fit() {
    let menu = [
        row(StrategyId::LowPower, Impact::None),
        row(StrategyId::HighPerformance, Impact::None),
    ];
    let holding = |micros: u64, gpu: u64| {
        Some(Cost {
            time: PerDevice {
                cpu: Duration::from_micros(micros),
                ..PerDevice::default()
            },
            memory: PerDevice {
                gpu,
                ..PerDevice::default()
            },
            ..Cost::default()
        })
    };
    let envelope = Envelope {
        memory: PerDevice {
            gpu: 1000,
            ..ANY_MEMORY
        },
        ..host_envelope(400)
    };

    assert_eq!(
        chosen(&envelope, &[holding(100, 0), holding(200, 2000)], &menu, 0),
        within(0),
        "the dearer row holds more graphics memory than the envelope lends"
    );
}

/// **Nothing fits: the floor runs, and says so.** The floor is the cheapest
/// priced row — here the third of the menu, not its first (unpriced) nor its
/// default level.
#[test]
fn nothing_fits_the_floor_runs_and_says_over_budget() {
    let menu = [
        row(StrategyId::Custom(9), Impact::None),
        row(StrategyId::HighPerformance, Impact::None),
        row(StrategyId::LowPower, Impact::Major),
        row(StrategyId::Balanced, Impact::Minor),
    ];
    let quotes = [None, host(300), host(100), host(200)];

    assert_eq!(
        chosen(&host_envelope(50), &quotes, &menu, 1),
        Choice {
            row: 2,
            over_budget: true,
        },
        "the cheapest priced row, flagged over budget"
    );
}

/// **Nothing priced: the default level runs.** No row has a quote to fit the
/// envelope with, so the family runs the level its registration names, and
/// nothing says it is over budget.
#[test]
fn nothing_priced_the_default_level_runs() {
    let quotes = [None, None, None, None, None, None];

    assert_eq!(
        chosen(&host_envelope(400), &quotes, MENU, 4),
        within(4),
        "the default level"
    );
    assert_eq!(
        chosen(&Envelope::default(), &quotes, MENU, 1),
        within(1),
        "whatever the envelope"
    );
}

/// **A pin overrides the choice, and an allowed list filters the rows.**
/// Pinned to the 900 µs row, the agent runs it within 400 µs; pinned with no
/// row priced, it runs the pin, not the default. Allowed only the Major row
/// and the cheap row without impact, within 400 µs it takes the latter, not
/// the 200 µs row it would take otherwise.
#[test]
fn a_pin_overrides_the_choice_and_allowed_rows_filter_it() {
    let quotes = menu_quotes();
    let envelope = host_envelope(400);

    let pinned = choose(
        &envelope,
        &quotes,
        MENU,
        MENU_LANES,
        0,
        Some(StrategyId::HighPerformance),
        None,
    );
    assert_eq!(pinned.row, 4, "the pin, though it does not fit");

    let pinned_unpriced = choose(
        &envelope,
        &[None, None, None, None, None, None],
        MENU,
        MENU_LANES,
        0,
        Some(StrategyId::Balanced),
        None,
    );
    assert_eq!(pinned_unpriced.row, 1, "the pin, though nothing is priced");

    let allowed = [StrategyId::LowPower, StrategyId::Custom(1)];
    assert_eq!(
        choose(
            &envelope,
            &quotes,
            MENU,
            MENU_LANES,
            0,
            None,
            Some(&allowed)
        ),
        within(2),
        "the best allowed row"
    );
}

/// A pin naming no row of the menu pins nothing: the choice is made as if
/// there were none.
#[test]
fn a_pin_naming_no_row_is_ignored() {
    assert_eq!(
        choose(
            &host_envelope(400),
            &menu_quotes(),
            MENU,
            MENU_LANES,
            0,
            Some(StrategyId::Custom(42)),
            None,
        ),
        within(3)
    );
}

/// **The floor is taken among the allowed rows.** Allowed only the 300 µs and
/// 900 µs rows, nothing allowed fits 120 µs: the cheapest allowed row runs,
/// over budget — not the Major row, which fits but is not allowed.
#[test]
fn when_no_allowed_row_fits_the_floor_is_the_cheapest_allowed_row() {
    let allowed = [StrategyId::Balanced, StrategyId::HighPerformance];

    assert_eq!(
        choose(
            &host_envelope(120),
            &menu_quotes(),
            MENU,
            MENU_LANES,
            0,
            None,
            Some(&allowed)
        ),
        Choice {
            row: 1,
            over_budget: true,
        }
    );
}

/// **Nothing allowed is priced: an allowed row runs, never one the policy
/// excludes.** Allowed only the unpriced row, the agent has no allowed price
/// to fit; it must not fall back to its default level, which the policy does
/// not allow.
#[test]
fn when_no_allowed_row_is_priced_the_row_that_runs_is_still_allowed() {
    let allowed = [StrategyId::Custom(3)];

    let choice = choose(
        &host_envelope(400),
        &menu_quotes(),
        MENU,
        MENU_LANES,
        0,
        None,
        Some(&allowed),
    );

    assert!(
        allowed.contains(&MENU[choice.row].id),
        "row {} ({:?}) runs, which the policy does not allow",
        choice.row,
        MENU[choice.row].id
    );
}
