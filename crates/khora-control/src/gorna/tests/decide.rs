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

//! How `decide` shares the frame: admission of the floors, Q-RAM upgrades,
//! the 80 / 20 slack, the policies as envelope shapes, and determinism.
//! Every figure is in whole microseconds, so the expected envelopes are exact.

use super::*;
use khora_core::agent::gorna::protocol::Trigger;
use khora_core::lane::cost::PerLink;

use AgentImportance::{Critical, Important, Optional};
use StrategyId::{Balanced, Custom, HighPerformance, LowPower};

/// `offer` as weighed by `importance`, under no policy, holding nothing.
fn bid(offer: Offer, importance: AgentImportance) -> Bid {
    Bid {
        offer,
        importance,
        mode: AdaptationMode::Learning,
        hints: AgentHints::default(),
        kept: None,
        late: false,
    }
}

/// A round opened by `trigger`, of `target_us` on the critical path, with no
/// memory figure, untimed links and every agent running alone.
fn round<'a>(trigger: Trigger, target_us: u64, bids: &'a [Bid]) -> Round<'a> {
    Round {
        epoch: Epoch(7),
        trigger,
        target: us(target_us),
        memory: PerDevice::default(),
        link_rate: PerLink::default(),
        waves: &[],
        bids,
    }
}

/// `cpu_us` on the host and no time elsewhere.
fn on_cpu(cpu_us: u64) -> PerDevice<Duration> {
    cpu_envelope(cpu_us).time
}

/// The envelopes sorted by agent, to compare allocations whatever order the
/// bids came in.
fn sorted(allocation: &Allocation) -> Vec<(AgentId, Envelope)> {
    let mut envelopes = allocation.envelopes.clone();
    envelopes.sort_by_key(|(agent, _)| *agent);
    envelopes
}

/// The frame's time on one device: each wave costs its slowest member, an
/// agent in no wave costs its own.
fn critical_path(
    allocation: &Allocation,
    waves: &[Vec<AgentId>],
    device: fn(&PerDevice<Duration>) -> Duration,
) -> Duration {
    let time_of = |agent: &AgentId| {
        allocation
            .envelopes
            .iter()
            .find(|(id, _)| id == agent)
            .map_or(Duration::ZERO, |(_, envelope)| device(&envelope.time))
    };
    let in_a_wave = |agent: &AgentId| waves.iter().any(|wave| wave.contains(agent));
    let waves_time: Duration = waves
        .iter()
        .map(|wave| wave.iter().map(time_of).max().unwrap_or_default())
        .sum();
    let alone: Duration = allocation
        .envelopes
        .iter()
        .filter(|(agent, _)| !in_a_wave(agent))
        .map(|(agent, _)| time_of(agent))
        .sum();
    waves_time + alone
}

// ── Admission ─────────────────────────────────────────────────────

/// **The floors come first.** Floors of 4 + 5 ms fit 10 ms: the round keeps
/// its trigger and every envelope holds at least its agent's floor. Floors of
/// 6 + 5 ms do not: the round is `Safety`, and every envelope is exactly its
/// agent's floor, whatever else the agent offers.
#[test]
fn floors_are_admitted_first_or_the_round_is_safety() {
    let admitted = [
        bid(
            offer(
                "ash",
                vec![row(LowPower, 4_000, 0.4), row(HighPerformance, 9_000, 1.0)],
                LowPower,
            ),
            Important,
        ),
        bid(
            offer("birch", vec![row(LowPower, 5_000, 0.4)], LowPower),
            Important,
        ),
    ];
    let allocation = decide(&round(Trigger::Pressure, 10_000, &admitted));
    assert_eq!(allocation.trigger, Trigger::Pressure, "the floors fit");
    assert!(lent_to(&allocation, "ash").time.cpu >= us(4_000));
    assert!(lent_to(&allocation, "birch").time.cpu >= us(5_000));

    let refused = [
        bid(
            offer(
                "ash",
                vec![
                    row_holding(LowPower, 6_000, 3 * MIB, 0.4),
                    row(HighPerformance, 9_000, 1.0),
                ],
                LowPower,
            ),
            Important,
        ),
        bid(
            offer("birch", vec![row(LowPower, 5_000, 0.4)], LowPower),
            Important,
        ),
    ];
    let allocation = decide(&round(Trigger::Pressure, 10_000, &refused));
    assert_eq!(allocation.trigger, Trigger::Safety, "6 + 5 ms exceed 10 ms");
    assert_eq!(
        lent_to(&allocation, "ash"),
        envelope_of(&refused[0].offer.rows[0].cost),
        "the floor, time and memory"
    );
    assert_eq!(
        lent_to(&allocation, "birch"),
        envelope_of(&refused[1].offer.rows[0].cost)
    );
}

/// **A wave costs its slowest member.** The same 6 + 5 ms floors that a
/// serial frame refuses fit 10 ms once the two agents share a wave.
#[test]
fn admission_counts_a_wave_by_its_slowest_member() {
    let bids = [
        bid(
            offer("ash", vec![row(LowPower, 6_000, 0.4)], LowPower),
            Important,
        ),
        bid(
            offer("birch", vec![row(LowPower, 5_000, 0.4)], LowPower),
            Important,
        ),
    ];
    let waves = [vec![AgentId::new("ash"), AgentId::new("birch")]];
    let allocation = decide(&Round {
        waves: &waves,
        ..round(Trigger::Pressure, 10_000, &bids)
    });
    assert_eq!(
        allocation.trigger,
        Trigger::Pressure,
        "max(6, 5) ms fit 10 ms"
    );
}

/// **Transfers are on the critical path.** Floors of 4 + 4 ms fit 10 ms,
/// but not once `ash` sends 3 MB to the graphics processor over a link moving
/// 1 GB/s: 3 ms more. A link the DCC cannot time (rate 0) adds nothing.
#[test]
fn admission_counts_the_transfers() {
    let mut sending = row(LowPower, 4_000, 0.4);
    sending.cost.transfer.cpu_to_gpu = 3_000_000;
    let bids = [
        bid(offer("ash", vec![sending], LowPower), Important),
        bid(
            offer("birch", vec![row(LowPower, 4_000, 0.4)], LowPower),
            Important,
        ),
    ];

    let timed = decide(&Round {
        link_rate: PerLink {
            cpu_to_gpu: 1_000_000_000,
            gpu_to_cpu: 1_000_000_000,
        },
        ..round(Trigger::Pressure, 10_000, &bids)
    });
    assert_eq!(timed.trigger, Trigger::Safety, "4 + 4 + 3 ms exceed 10 ms");

    let untimed = decide(&round(Trigger::Pressure, 10_000, &bids));
    assert_eq!(
        untimed.trigger,
        Trigger::Pressure,
        "an untimed link adds nothing"
    );
}

/// **The emergency names no row.** A `Safety` round lends every agent its
/// floor even when far more would fit, pins nothing and allows nothing —
/// whether the floor is called `LowPower`, `Balanced` or `Custom`.
#[test]
fn a_safety_round_gives_every_agent_its_floor_even_when_more_fits() {
    let bids = [
        bid(
            offer(
                "grid",
                vec![row(Custom(2), 3_000, 1.0), row(Custom(1), 1_000, 0.4)],
                Custom(1),
            ),
            Optional,
        ),
        bid(
            offer(
                "skybox",
                vec![row(Balanced, 2_000, 0.7), row(HighPerformance, 5_000, 1.0)],
                Balanced,
            ),
            Important,
        ),
        bid(
            offer(
                "renderer",
                vec![row(LowPower, 1_500, 0.4), row(Balanced, 4_000, 1.0)],
                LowPower,
            ),
            Critical,
        ),
    ];
    let allocation = decide(&round(Trigger::Safety, 100_000, &bids));

    assert_eq!(allocation.trigger, Trigger::Safety);
    assert_eq!(lent_to(&allocation, "grid"), cpu_envelope(1_000));
    assert_eq!(lent_to(&allocation, "skybox"), cpu_envelope(2_000));
    assert_eq!(lent_to(&allocation, "renderer"), cpu_envelope(1_500));
    assert!(allocation.pins.is_empty(), "{:?}", allocation.pins);
    assert!(allocation.allowed.is_empty(), "{:?}", allocation.allowed);
}

#[test]
fn a_round_without_agents_lends_nothing() {
    let allocation = decide(&round(Trigger::Initial, 16_000, &[]));
    assert!(allocation.envelopes.is_empty());
    assert_eq!(allocation.epoch, Epoch(7), "the round's epoch");
    assert_eq!(allocation.trigger, Trigger::Initial);
}

// ── Upgrades ──────────────────────────────────────────────────────

/// **Q-RAM order: quality gained per microsecond, weighed by importance.**
/// Either upgrade fits 5 ms alone, both do not. `oak` (Critical) gains
/// 1.0 × 0.6 over 3 ms, 0.0002 per µs; `pine` (Optional) gains 0.5 × 0.6
/// over 0.5 ms, 0.0006 per µs. `pine` is upgraded first, and `oak`'s upgrade
/// no longer fits: the less important agent wins because its gain per µs is
/// higher. Then the weights decide: with rows whose raw gain per µs favours
/// `pine`, `oak` takes the upgrade once it is Critical and `pine` Optional,
/// and `pine` takes it back once the importances swap.
#[test]
fn upgrades_go_to_the_best_gain_per_microsecond() {
    // The gain per µs outranks the importance.
    let bids = [
        bid(
            offer(
                "oak",
                vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 4_000, 1.0)],
                LowPower,
            ),
            Critical,
        ),
        bid(
            offer(
                "pine",
                vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 1_500, 1.0)],
                LowPower,
            ),
            Optional,
        ),
    ];
    let allocation = decide(&round(Trigger::Refresh, 5_000, &bids));
    assert_eq!(allocation.trigger, Trigger::Refresh);
    // 1 000 + 1 500 µs planned, 2 500 µs slack, 2 000 shared in proportion.
    assert_eq!(lent_to(&allocation, "oak").time, on_cpu(1_800));
    assert_eq!(lent_to(&allocation, "pine").time, on_cpu(2_700));

    // Raw gain per µs: oak 0.6 / 2 000, pine 0.6 / 1 500. Either upgrade fits
    // 4.5 ms alone, both do not.
    let pair = |oak: AgentImportance, pine: AgentImportance| {
        [
            bid(
                offer(
                    "oak",
                    vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 3_000, 1.0)],
                    LowPower,
                ),
                oak,
            ),
            bid(
                offer(
                    "pine",
                    vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 2_500, 1.0)],
                    LowPower,
                ),
                pine,
            ),
        ]
    };

    // Weighed, oak gains 0.0003 per µs and pine 0.0002: oak is upgraded.
    let weighed = pair(Critical, Optional);
    let allocation = decide(&round(Trigger::Refresh, 4_500, &weighed));
    let (oak, pine) = (lent_to(&allocation, "oak"), lent_to(&allocation, "pine"));
    assert!(
        fits(&weighed[0].offer.rows[1].cost, &oak),
        "oak upgraded: {oak:?}"
    );
    assert!(
        !fits(&weighed[1].offer.rows[1].cost, &pine),
        "pine stays: {pine:?}"
    );

    // Swapped, pine gains 0.0004 per µs and oak 0.00015: pine is upgraded.
    let swapped = pair(Optional, Critical);
    let allocation = decide(&round(Trigger::Refresh, 4_500, &swapped));
    let (oak, pine) = (lent_to(&allocation, "oak"), lent_to(&allocation, "pine"));
    assert!(
        !fits(&swapped[0].offer.rows[1].cost, &oak),
        "oak stays: {oak:?}"
    );
    assert!(
        fits(&swapped[1].offer.rows[1].cost, &pine),
        "pine upgraded: {pine:?}"
    );
}

/// **Upgrades climb each agent's convex hull.** `oak`'s middle row is a poor
/// step (0.1 for 2 ms) under the line to its top row (0.6 for 3 ms, 0.0002
/// per µs), so the hull goes straight to the top, which outranks `pine`'s
/// 0.0001 per µs and leaves no room for it. Stepping row by row would have
/// upgraded `pine` first and stopped `oak` at its middle row.
#[test]
fn upgrades_climb_each_agents_convex_hull() {
    let bids = [
        bid(
            offer(
                "oak",
                vec![
                    row(LowPower, 1_000, 0.4),
                    row(Balanced, 3_000, 0.5),
                    row(HighPerformance, 4_000, 1.0),
                ],
                LowPower,
            ),
            Critical,
        ),
        bid(
            offer(
                "pine",
                vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 2_000, 0.5)],
                LowPower,
            ),
            Critical,
        ),
    ];
    let allocation = decide(&round(Trigger::Refresh, 5_200, &bids));
    // 4 000 + 1 000 µs planned, 200 µs slack, 160 shared in proportion.
    assert_eq!(lent_to(&allocation, "oak").time, on_cpu(4_128));
    assert_eq!(lent_to(&allocation, "pine").time, on_cpu(1_032));
}

/// **A row without a price is never planned.** `ash`'s top row is `Assumed`:
/// no time, so it would look free. Planning it would lend `ash` nothing to
/// run its floor with; its envelope holds its floor.
#[test]
fn an_unpriced_row_is_never_planned() {
    let assumed = OfferRow {
        id: HighPerformance,
        cost: Cost::default(),
        quality: 1.0,
        confidence: Confidence::Assumed,
    };
    let bids = [
        bid(
            offer("ash", vec![row(LowPower, 2_000, 0.4), assumed], LowPower),
            Important,
        ),
        bid(
            offer("birch", vec![row(LowPower, 4_000, 0.4)], LowPower),
            Important,
        ),
    ];
    let allocation = decide(&round(Trigger::Refresh, 16_000, &bids));
    assert!(
        lent_to(&allocation, "ash").time.cpu >= us(2_000),
        "{allocation:?}"
    );
}

/// **An upgrade must fit the memory too.** 16 ms fit `ash`'s top row (14 ms)
/// but its 20 MB do not fit the 15 MB the graphics processor has: the middle
/// row (10 MB) is planned, and the envelope lends 10 MB plus 80 % of the
/// 5 MB left — not enough for the top row, though its time would fit.
#[test]
fn an_upgrade_that_does_not_fit_the_memory_is_not_taken() {
    const MB: u64 = 1_000_000;
    let bids = [bid(
        offer(
            "ash",
            vec![
                row_holding(LowPower, 2_000, MB, 0.4),
                row_holding(Balanced, 8_000, 10 * MB, 0.8),
                row_holding(HighPerformance, 14_000, 20 * MB, 1.0),
            ],
            LowPower,
        ),
        Important,
    )];
    let allocation = decide(&Round {
        memory: PerDevice {
            gpu: Some(15 * MB),
            ..PerDevice::default()
        },
        ..round(Trigger::Refresh, 16_000, &bids)
    });
    let ash = lent_to(&allocation, "ash");
    assert_eq!(
        ash.time.cpu,
        us(14_400),
        "8 ms planned + 80 % of the 8 ms left"
    );
    assert_eq!(
        ash.memory.gpu,
        14 * MB,
        "10 MB planned + 80 % of the 5 MB left"
    );
    assert!(!fits(&bids[0].offer.rows[2].cost, &ash));
    assert!(fits(&bids[0].offer.rows[1].cost, &ash));
}

/// **A memory the DCC does not know limits nothing**: every row's memory
/// fits the envelope.
#[test]
fn an_unknown_memory_limits_nothing() {
    let bids = [bid(
        offer(
            "ash",
            vec![
                row_holding(LowPower, 2_000, MIB, 0.4),
                row_holding(HighPerformance, 3_000, 512 * MIB, 1.0),
            ],
            LowPower,
        ),
        Important,
    )];
    let allocation = decide(&round(Trigger::Refresh, 16_000, &bids));
    let ash = lent_to(&allocation, "ash");
    assert!(ash.memory.gpu >= 512 * MIB, "{ash:?}");
    assert!(fits(&bids[0].offer.rows[1].cost, &ash));
}

// ── The share ─────────────────────────────────────────────────────

/// **80 % of the slack goes back to the envelopes, in proportion; 20 % stays
/// in reserve.** 2 + 6 ms planned in 16 ms leave 8 ms: 6.4 ms are shared
/// 1 : 3, so 3.6 ms and 10.8 ms, and 1.6 ms stay unlent.
#[test]
fn slack_is_shared_eighty_twenty() {
    let bids = [
        bid(
            offer("ash", vec![row(LowPower, 2_000, 0.4)], LowPower),
            Important,
        ),
        bid(
            offer("birch", vec![row(LowPower, 6_000, 0.4)], LowPower),
            Critical,
        ),
    ];
    let allocation = decide(&round(Trigger::Refresh, 16_000, &bids));
    assert_eq!(lent_to(&allocation, "ash").time, on_cpu(3_600));
    assert_eq!(lent_to(&allocation, "birch").time, on_cpu(10_800));
    let lent: Duration = allocation.envelopes.iter().map(|(_, e)| e.time.cpu).sum();
    assert_eq!(us(16_000) - lent, us(1_600), "the reserve");
}

/// **The memory's slack is shared the same way.** 100 + 300 MB planned of
/// 1 000 MB leave 600 MB: 480 MB are shared 1 : 3.
#[test]
fn memory_slack_is_shared_like_time() {
    const MB: u64 = 1_000_000;
    let bids = [
        bid(
            offer(
                "ash",
                vec![row_holding(LowPower, 2_000, 100 * MB, 0.4)],
                LowPower,
            ),
            Important,
        ),
        bid(
            offer(
                "birch",
                vec![row_holding(LowPower, 6_000, 300 * MB, 0.4)],
                LowPower,
            ),
            Important,
        ),
    ];
    let allocation = decide(&Round {
        memory: PerDevice {
            gpu: Some(1_000 * MB),
            ..PerDevice::default()
        },
        ..round(Trigger::Refresh, 16_000, &bids)
    });
    assert_eq!(lent_to(&allocation, "ash").memory.gpu, 220 * MB);
    assert_eq!(lent_to(&allocation, "birch").memory.gpu, 660 * MB);
}

/// A tiny deterministic generator, so the property below sees many rounds.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, below: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % below
    }
}

/// **I2: the envelopes fit the target on the critical path, on every
/// device, and the memory they lend fits the memory there is.** Over many
/// rounds of four agents with host and graphics rows, in waves and alone.
#[test]
fn envelopes_sum_within_the_target_on_the_critical_path() {
    const NAMES: [&str; 4] = ["ash", "birch", "oak", "pine"];
    let wave_plans: [Vec<Vec<AgentId>>; 3] = [
        Vec::new(),
        vec![
            vec![AgentId::new("ash"), AgentId::new("birch")],
            vec![AgentId::new("oak")],
        ],
        vec![vec![
            AgentId::new("ash"),
            AgentId::new("birch"),
            AgentId::new("oak"),
            AgentId::new("pine"),
        ]],
    ];
    let mut lcg = Lcg(0x5eed);
    for case in 0..60 {
        let importances = [Critical, Important, Optional];
        let bids: Vec<Bid> = NAMES
            .iter()
            .map(|name| {
                let floor = OfferRow {
                    id: LowPower,
                    cost: Cost {
                        time: PerDevice {
                            cpu: us(100 + lcg.next(900)),
                            gpu: us(lcg.next(900)),
                            ..PerDevice::default()
                        },
                        memory: PerDevice {
                            gpu: MIB + lcg.next(8) * MIB,
                            ..PerDevice::default()
                        },
                        ..Cost::default()
                    },
                    quality: 0.4,
                    confidence: Confidence::Quoted,
                };
                let mut rows = vec![floor];
                for (index, quality) in [0.7_f32, 1.0].into_iter().enumerate() {
                    let mut up = floor;
                    up.id = Custom(index as u32);
                    up.quality = quality;
                    up.cost.time.cpu += us(lcg.next(8_000));
                    up.cost.time.gpu += us(lcg.next(6_000));
                    up.cost.memory.gpu += lcg.next(64) * MIB;
                    rows.push(up);
                }
                let importance = importances[lcg.next(3) as usize];
                bid(offer(name, rows, LowPower), importance)
            })
            .collect();
        let waves = &wave_plans[case % wave_plans.len()];
        let target = us(16_667);
        let memory = Some(160 * MIB);
        let allocation = decide(&Round {
            waves,
            memory: PerDevice {
                gpu: memory,
                ..PerDevice::default()
            },
            ..round(Trigger::Refresh, target.as_micros() as u64, &bids)
        });

        assert_ne!(
            allocation.trigger,
            Trigger::Safety,
            "case {case}: floors fit"
        );
        assert_eq!(allocation.envelopes.len(), NAMES.len(), "case {case}");
        let cpu = critical_path(&allocation, waves, |t| t.cpu);
        let gpu = critical_path(&allocation, waves, |t| t.gpu);
        assert!(cpu <= target, "case {case}: host path {cpu:?} > {target:?}");
        assert!(
            gpu <= target,
            "case {case}: graphics path {gpu:?} > {target:?}"
        );
        let held: u64 = allocation.envelopes.iter().map(|(_, e)| e.memory.gpu).sum();
        assert!(held <= 160 * MIB, "case {case}: {held} bytes lent");
        for bid in &bids {
            let lent = lent_to(&allocation, bid.offer.agent.name());
            assert!(
                fits(&bid.offer.rows[0].cost, &lent),
                "case {case}: {} cannot run its floor in {lent:?}",
                bid.offer.agent
            );
        }
    }
}

// ── Determinism ───────────────────────────────────────────────────

/// **I6: the same round gives the same allocation**, from one call to the
/// next and whatever order the agents are listed in.
#[test]
fn same_inputs_same_allocation() {
    let bids = vec![
        bid(
            offer(
                "renderer",
                vec![
                    row(LowPower, 1_000, 0.4),
                    row(Balanced, 3_333, 0.7),
                    row(HighPerformance, 6_667, 1.0),
                ],
                LowPower,
            ),
            Critical,
        ),
        bid(
            offer(
                "shadows",
                vec![row(LowPower, 700, 0.4), row(HighPerformance, 2_100, 1.0)],
                LowPower,
            ),
            Important,
        ),
        bid(
            offer(
                "grid",
                vec![row(Balanced, 300, 0.7), row(HighPerformance, 900, 1.0)],
                Balanced,
            ),
            Optional,
        ),
        bid(
            offer(
                "gizmo",
                vec![row(Balanced, 300, 0.7), row(HighPerformance, 900, 1.0)],
                Balanced,
            ),
            Optional,
        ),
    ];
    let target = Duration::from_nanos(16_666_667);
    let first = decide(&Round {
        target,
        ..round(Trigger::Refresh, 0, &bids)
    });
    let again = decide(&Round {
        target,
        ..round(Trigger::Refresh, 0, &bids)
    });
    assert_eq!(first, again, "one round, one allocation");

    let reversed: Vec<Bid> = bids.iter().rev().cloned().collect();
    let other_order = decide(&Round {
        target,
        ..round(Trigger::Refresh, 0, &reversed)
    });
    assert_eq!(sorted(&first), sorted(&other_order), "whatever the order");
}

/// **A tie goes to the agent named first.** Two identical agents, room for
/// one upgrade: `alpha` takes it, whichever is listed first.
#[test]
fn a_tie_goes_to_the_agent_named_first() {
    let twin = |name| {
        bid(
            offer(
                name,
                vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 3_000, 1.0)],
                LowPower,
            ),
            Important,
        )
    };
    for bids in [[twin("alpha"), twin("beta")], [twin("beta"), twin("alpha")]] {
        let allocation = decide(&round(Trigger::Refresh, 5_000, &bids));
        let top = cpu_cost(3_000);
        assert!(fits(&top, &lent_to(&allocation, "alpha")), "{allocation:?}");
        assert!(!fits(&top, &lent_to(&allocation, "beta")), "{allocation:?}");
    }
}

// ── Policies are envelope shapes ──────────────────────────────────

/// **`Cap` is a ceiling on the envelope.** Uncapped, `ash` alone in 16 ms
/// plans its top row; capped at 5 ms, its envelope stays within 5 ms on every
/// device, and still holds the dearest row under the cap.
#[test]
fn a_cap_is_a_ceiling_on_the_envelope() {
    let rows = vec![
        row(LowPower, 2_000, 0.4),
        row(Balanced, 4_000, 0.7),
        row(HighPerformance, 9_000, 1.0),
    ];
    let free = [bid(offer("ash", rows.clone(), LowPower), Important)];
    let allocation = decide(&round(Trigger::Refresh, 16_000, &free));
    assert!(busiest(&lent_to(&allocation, "ash")) > ms(5), "uncapped");

    let mut capped = bid(offer("ash", rows, LowPower), Important);
    capped.hints.cap_ms = Some(5.0);
    let capped = [capped];
    let allocation = decide(&round(Trigger::Refresh, 16_000, &capped));
    let ash = lent_to(&allocation, "ash");
    assert!(busiest(&ash) <= ms(5), "capped at 5 ms: {ash:?}");
    assert!(
        fits(&cpu_cost(4_000), &ash),
        "the middle row is under the cap"
    );
}

/// **`Manual(row)` is a pin**: the allocation pins the agent to its row and
/// lends it enough to run it.
#[test]
fn a_manual_mode_is_a_pin() {
    let mut pinned = bid(
        offer(
            "ash",
            vec![
                row(LowPower, 2_000, 0.4),
                row(Balanced, 6_000, 0.7),
                row(HighPerformance, 9_000, 1.0),
            ],
            LowPower,
        ),
        Important,
    );
    pinned.mode = AdaptationMode::Manual(Balanced);
    let bids = [
        pinned,
        bid(
            offer("birch", vec![row(LowPower, 2_000, 0.4)], LowPower),
            Critical,
        ),
    ];
    let allocation = decide(&round(Trigger::Refresh, 12_000, &bids));
    assert_eq!(allocation.pins, vec![(AgentId::new("ash"), Balanced)]);
    assert!(fits(&cpu_cost(6_000), &lent_to(&allocation, "ash")));
}

/// **`Bounded { min, max }` is an allowed range** of the agent's rows.
#[test]
fn a_bounded_mode_is_an_allowed_range() {
    let mut bounded = bid(
        offer(
            "ash",
            vec![
                row(LowPower, 2_000, 0.4),
                row(Balanced, 4_000, 0.7),
                row(HighPerformance, 9_000, 1.0),
            ],
            LowPower,
        ),
        Important,
    );
    bounded.mode = AdaptationMode::Bounded {
        min: LowPower,
        max: Balanced,
    };
    let bids = [bounded];
    let allocation = decide(&round(Trigger::Refresh, 16_000, &bids));
    let allowed: Vec<_> = allocation
        .allowed
        .iter()
        .filter(|(agent, _)| *agent == AgentId::new("ash"))
        .collect();
    assert_eq!(allowed.len(), 1, "{:?}", allocation.allowed);
    let mut rows = allowed[0].1.clone();
    rows.sort_by_key(|id| format!("{id:?}"));
    assert_eq!(rows, vec![Balanced, LowPower]);
    assert!(allocation.pins.is_empty());
}

/// **`Stable` keeps the envelope** when the frame would lend it more, and
/// gives way when the envelope it holds no longer fits.
#[test]
fn a_stable_agent_keeps_its_envelope_unless_it_no_longer_fits() {
    let stable = |kept_us| {
        let mut stable = bid(
            offer("ash", vec![row(LowPower, 2_000, 0.4)], LowPower),
            Important,
        );
        stable.mode = AdaptationMode::Stable;
        stable.kept = Some(cpu_envelope(kept_us));
        [stable]
    };

    let roomier = stable(5_000);
    let allocation = decide(&round(Trigger::PidDrift, 16_000, &roomier));
    assert_eq!(lent_to(&allocation, "ash"), cpu_envelope(5_000), "kept");

    let tighter = stable(12_000);
    let allocation = decide(&round(Trigger::Pressure, 8_000, &tighter));
    let ash = lent_to(&allocation, "ash");
    assert!(ash.time.cpu <= us(8_000), "within the frame: {ash:?}");
    assert!(ash.time.cpu >= us(2_000), "still its floor: {ash:?}");
}

/// **A late bid keeps the envelope it held, and is named.** `birch` answered
/// after `reply_by`: its rows are not read, its envelope is the one it held,
/// and the allocation says so; `ash`, on time, is not named.
#[test]
fn a_late_bid_keeps_its_envelope_and_is_named() {
    let mut late = bid(
        offer(
            "birch",
            vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 9_000, 1.0)],
            LowPower,
        ),
        Important,
    );
    late.late = true;
    late.kept = Some(cpu_envelope(4_321));
    let bids = [
        bid(
            offer("ash", vec![row(LowPower, 2_000, 0.4)], LowPower),
            Important,
        ),
        late,
    ];
    let allocation = decide(&round(Trigger::Refresh, 16_000, &bids));
    assert_eq!(lent_to(&allocation, "birch"), cpu_envelope(4_321));
    assert_eq!(allocation.late, vec![AgentId::new("birch")]);
    let ash = lent_to(&allocation, "ash");
    assert!(
        ash.time.cpu + us(4_321) <= us(16_000),
        "the kept envelope counts in the frame: {ash:?}"
    );
}

/// **A `Stable` agent never keeps an envelope below its floor.** `ash` holds
/// 1 ms from an earlier round, but its floor now costs 8 ms: keeping 1 ms
/// would lend it less than it can run, and lend `birch` the time `ash`'s
/// floor needs. Whatever `ash` is lent holds its floor, and the two
/// envelopes, with `ash` at its floor, stay within the 16 ms frame.
#[test]
fn a_stable_envelope_below_the_floor_is_not_kept() {
    let mut stable = bid(
        offer("ash", vec![row(LowPower, 8_000, 0.4)], LowPower),
        Important,
    );
    stable.mode = AdaptationMode::Stable;
    stable.kept = Some(cpu_envelope(1_000));
    let bids = [
        stable,
        bid(
            offer("birch", vec![row(LowPower, 5_000, 0.4)], LowPower),
            Important,
        ),
    ];
    let allocation = decide(&round(Trigger::Pressure, 16_000, &bids));
    assert_ne!(allocation.trigger, Trigger::Safety, "8 + 5 ms fit 16 ms");
    let ash = lent_to(&allocation, "ash");
    let birch = lent_to(&allocation, "birch");
    assert!(ash.time.cpu >= us(8_000), "at least its floor: {ash:?}");
    assert!(
        us(8_000) + birch.time.cpu <= us(16_000),
        "birch is not lent the time ash's floor runs in: {birch:?}"
    );
}

/// **A `Bounded` agent whose range leaves out its floor is planned at a row
/// it may run.** `ash` may run only `Balanced` (4 ms); its floor `LowPower`
/// (1 ms) is outside the range. `birch`'s upgrade is steeper, but `ash`'s
/// cheapest allowed row and `birch`'s floor fit the 6 ms frame (4 + 1 ms):
/// `ash` is lent room for `Balanced`, not an envelope no allowed row fits.
#[test]
fn a_bounded_agent_is_lent_room_for_an_allowed_row() {
    let mut bounded = bid(
        offer(
            "ash",
            vec![row(LowPower, 1_000, 0.4), row(Balanced, 4_000, 0.7)],
            LowPower,
        ),
        Important,
    );
    bounded.mode = AdaptationMode::Bounded {
        min: Balanced,
        max: HighPerformance,
    };
    let bids = [
        bounded,
        bid(
            offer(
                "birch",
                vec![row(LowPower, 1_000, 0.4), row(HighPerformance, 4_000, 1.0)],
                LowPower,
            ),
            Critical,
        ),
    ];
    let allocation = decide(&round(Trigger::Refresh, 6_000, &bids));
    assert_ne!(allocation.trigger, Trigger::Safety, "4 + 1 ms fit 6 ms");
    let ash = lent_to(&allocation, "ash");
    assert!(
        ash.time.cpu >= us(4_000),
        "room for `Balanced`, its only allowed row: {ash:?}"
    );
    assert!(
        critical_path(&allocation, &[], |time| time.cpu) <= us(6_000),
        "{allocation:?}"
    );
}
