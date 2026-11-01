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

//! How much an agent weighs in the share: its declared importance, or a
//! `Prioritize` hint in its place — never its name.

use super::*;
use khora_core::agent::gorna::protocol::Trigger;

use AgentImportance::{Critical, Important, Optional};
use StrategyId::{Balanced, LowPower};

/// `name` weighing `importance`, offering a 2 ms floor of quality 0.4 and an
/// 8 ms row of quality 1.0.
fn contender(name: &'static str, importance: AgentImportance) -> Bidder {
    Bidder::new(
        name,
        vec![row(LowPower, 2_000, 0.4), row(Balanced, 8_000, 1.0)],
        LowPower,
    )
    .weighing(importance)
}

/// Whether `name`'s envelope holds its 8 ms row.
fn upgraded(allocation: &Allocation, name: &'static str) -> bool {
    fits(&cpu_cost(8_000), &lent_to(allocation, name))
}

/// One round of 10.5 ms: both 2 ms floors fit, and one upgrade to 8 ms.
fn share(agents: &[Arc<Mutex<dyn Agent>>], hints: &HashMap<AgentId, AgentHints>) -> Allocation {
    arbitrator().allocate(
        &playing(),
        &frame_of(10.5),
        Trigger::Refresh,
        agents,
        &nothing(),
        hints,
    )
}

/// **Priority comes from importance, not from a table of names.** With room
/// for one upgrade, the more important agent takes it, whatever the two are
/// called and in whichever order they are listed. The names are the ones an
/// old table ranked the other way round (`renderer` above `asset`).
#[test]
fn priority_comes_from_importance_not_from_a_table() {
    let pairs = [
        (Critical, Optional),
        (Critical, Important),
        (Important, Optional),
    ];
    let namings = [("asset", "renderer"), ("renderer", "asset")];

    for (higher, lower) in pairs {
        for (higher_name, lower_name) in namings {
            for higher_first in [true, false] {
                let case = format!(
                    "{higher:?} `{higher_name}` vs {lower:?} `{lower_name}`, \
                     the {higher:?} one listed {}",
                    if higher_first { "first" } else { "second" }
                );
                let mut agents = vec![
                    shared(contender(higher_name, higher)).1,
                    shared(contender(lower_name, lower)).1,
                ];
                if !higher_first {
                    agents.reverse();
                }

                let allocation = share(&agents, &nothing());

                assert!(upgraded(&allocation, higher_name), "{case}: {allocation:?}");
                assert!(!upgraded(&allocation, lower_name), "{case}: {allocation:?}");
            }
        }
    }
}

/// **A `Prioritize` hint outweighs importance, both ways.** With room for
/// one upgrade, a `Critical` agent hinted down to 0 gives it to an unhinted
/// `Optional` one; an `Optional` agent hinted up to 2 takes it from an
/// unhinted `Critical` one. The hint replaces the weight importance gives.
#[test]
fn a_prioritize_hint_outweighs_importance() {
    for (hinted, hint, other, hinted_wins) in [
        (Critical, 0.0_f32, Optional, false),
        (Optional, 2.0_f32, Critical, true),
    ] {
        let case = format!("{hinted:?} hinted to {hint} against an unhinted {other:?}");
        let agents = [
            shared(contender("hinted", hinted)).1,
            shared(contender("unhinted", other)).1,
        ];
        let hints: HashMap<AgentId, AgentHints> = [(
            AgentId::new("hinted"),
            AgentHints {
                cap_ms: None,
                priority: Some(hint),
            },
        )]
        .into();

        let allocation = share(&agents, &hints);

        let (winner, loser) = if hinted_wins {
            ("hinted", "unhinted")
        } else {
            ("unhinted", "hinted")
        };
        assert!(upgraded(&allocation, winner), "{case}: {allocation:?}");
        assert!(!upgraded(&allocation, loser), "{case}: {allocation:?}");
    }
}
