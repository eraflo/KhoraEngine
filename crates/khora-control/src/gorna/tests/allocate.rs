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

//! A round of the arbitrator: one offer request per agent, calibration and
//! the decision — for agents that speak the protocol and agents that only
//! negotiate, through the bridge. A round lends nothing itself: the scheduler
//! lends the allocation at its next frame start. What an agent makes of its
//! envelope is checked here by lending it as the scheduler would ([`lend`]).

use super::*;
use khora_core::agent::gorna::protocol::Trigger;

use StrategyId::{Balanced, Custom, HighPerformance, LowPower};

/// One live round in `context`, for a frame of `latency_ms`.
fn allocate(
    arbitrator: &mut GornaArbitrator,
    context: &Context,
    latency_ms: f32,
    trigger: Trigger,
    agents: &[Arc<Mutex<dyn Agent>>],
) -> Allocation {
    arbitrator.allocate(
        context,
        &frame_of(latency_ms),
        trigger,
        agents,
        &nothing(),
        &nothing(),
    )
}

fn lock<A>(agent: &Arc<Mutex<A>>) -> std::sync::MutexGuard<'_, A> {
    agent.lock().expect("the agent lock")
}

// ── The round ─────────────────────────────────────────────────────

/// **A round asks each agent once, for its epoch, and lends nothing
/// itself.** The request carries the round's epoch, the frame's target (the
/// analysis' latency under the budget multiplier). The
/// round decides the agent's envelope but never calls its `apply` — the
/// scheduler lends it at its next frame start — nor its `negotiate` or
/// `apply_budget`. Each round opens a later epoch.
#[test]
fn a_round_asks_each_agent_once_and_lends_nothing_itself() {
    let (bidder, agent) = shared(Bidder::single("ash", 2_000));
    let mut arbitrator = arbitrator();
    let context = Context {
        global_budget_multiplier: 0.5,
        ..Default::default()
    };

    let first = allocate(
        &mut arbitrator,
        &context,
        20.0,
        Trigger::Initial,
        std::slice::from_ref(&agent),
    );
    {
        let bidder = lock(&bidder);
        assert_eq!(bidder.asked.len(), 1, "one offer request per round");
        let request = &bidder.asked[0];
        assert_eq!(request.epoch, first.epoch);
        assert!(
            request.target.abs_diff(ms(10)) < us(1),
            "20 ms × 0.5: {:?}",
            request.target
        );
        assert!(
            bidder.lent.is_empty(),
            "the round applied nothing: {:?}",
            bidder.lent
        );
        assert_eq!(bidder.old_calls, 0, "spoken to through the protocol only");
    }
    assert!(first
        .envelopes
        .iter()
        .any(|(id, _)| *id == AgentId::new("ash")));
    assert_eq!(first.trigger, Trigger::Initial);

    let second = allocate(&mut arbitrator, &context, 20.0, Trigger::Refresh, &[agent]);
    assert!(
        second.epoch > first.epoch,
        "{:?} then {:?}",
        first.epoch,
        second.epoch
    );
    assert_eq!(second.trigger, Trigger::Refresh);
    let bidder = lock(&bidder);
    assert_eq!(bidder.asked.len(), 2);
    assert_eq!(bidder.asked[1].epoch, second.epoch);
    assert!(bidder.lent.is_empty(), "still nothing applied");
}

/// **An agent that only negotiates is lent its envelope through the
/// bridge**: it is applied the dearest option the envelope holds, with the
/// envelope's host time as its limit. 2 ms planned in 20 ms leave room for
/// its 14 ms option.
#[test]
fn an_agent_that_only_negotiates_is_lent_an_envelope_through_the_bridge() {
    let (legacy, agent) = shared(Legacy::new("renderer"));
    let agents = [agent];
    let allocation = allocate(
        &mut arbitrator(),
        &playing(),
        20.0,
        Trigger::Initial,
        &agents,
    );
    lend(&allocation, &agents);
    let lent = lent_to(&allocation, "renderer");
    let legacy = lock(&legacy);
    assert_eq!(legacy.applied().time_limit, lent.time.cpu);
    assert_eq!(legacy.applied().strategy_id, HighPerformance, "{lent:?}");
}

#[test]
fn a_round_without_agents_lends_nothing() {
    let allocation = allocate(&mut arbitrator(), &playing(), 16.66, Trigger::Initial, &[]);
    assert!(allocation.envelopes.is_empty());
}

/// **The envelopes never exceed the frame**: two agents in 16.66 ms are lent
/// no more than 16.66 ms between them, and each still runs.
#[test]
fn envelopes_never_exceed_the_frame() {
    let (renderer, first) = shared(Legacy::new("renderer"));
    let (physics, second) = shared(Legacy::new("physics"));
    let agents = [first, second];
    let allocation = allocate(
        &mut arbitrator(),
        &playing(),
        16.66,
        Trigger::Initial,
        &agents,
    );
    lend(&allocation, &agents);
    let lent: Duration = allocation.envelopes.iter().map(|(_, e)| busiest(e)).sum();
    assert!(lent <= Duration::from_micros(16_660), "{allocation:?}");
    assert!(lock(&renderer).applied.is_some());
    assert!(lock(&physics).applied.is_some());
}

/// **A throttled frame lends less.** 33.33 ms under a 0.6 multiplier is
/// about 20 ms: the envelope fits it, and is smaller than at full budget.
#[test]
fn a_throttled_frame_shrinks_the_envelopes() {
    let lend = |multiplier: f32| {
        let (_, agent) = shared(Bidder::single("renderer", 2_000));
        let mut context = playing();
        context.hardware.thermal = khora_core::platform::ThermalStatus::Throttling;
        context.global_budget_multiplier = multiplier;
        let allocation = allocate(
            &mut arbitrator(),
            &context,
            33.33,
            Trigger::Pressure,
            &[agent],
        );
        busiest(&lent_to(&allocation, "renderer"))
    };
    let (throttled, full) = (lend(0.6), lend(1.0));
    assert!(throttled <= ms(20), "{throttled:?}");
    assert!(throttled < full, "{throttled:?} vs {full:?}");
}

/// **Concurrent agents share the frame by their slowest member.** Floors of
/// 6 ms each do not fit 10 ms in a row, so the round is `Safety`; once the
/// scheduler runs them in one wave they cost 6 ms, and they do.
#[test]
fn concurrent_agents_share_the_frame_by_their_slowest_member() {
    let two = || -> Vec<Arc<Mutex<dyn Agent>>> {
        vec![
            shared(Bidder::single("renderer", 6_000)).1,
            shared(Bidder::single("physics", 6_000)).1,
        ]
    };

    let serial = allocate(
        &mut arbitrator(),
        &playing(),
        10.0,
        Trigger::Refresh,
        &two(),
    );
    assert_eq!(serial.trigger, Trigger::Safety, "6 + 6 ms exceed 10 ms");

    let mut concurrent = arbitrator();
    concurrent.set_wave_plan(&[vec![AgentId::new("renderer"), AgentId::new("physics")]]);
    let waved = allocate(&mut concurrent, &playing(), 10.0, Trigger::Refresh, &two());
    assert_eq!(waved.trigger, Trigger::Refresh, "max(6, 6) ms fit 10 ms");
}

// ── Calibration ───────────────────────────────────────────────────

/// **Measured costs calibrate the offers.** `renderer` quotes its running
/// `Balanced` at 8 ms but measured 24 ms: every quoted row is scaled ×3, so
/// its floor costs 6 ms. With `physics`' 2 ms floor that is 8 ms, over the
/// 7 ms frame: the round is `Safety`, where the quotes alone (2 + 2 ms) fit.
#[test]
fn measured_costs_calibrate_the_offers() {
    let round = |measured: HashMap<AgentId, f64>| {
        let agents = [
            shared(Legacy::new("renderer")).1,
            shared(Legacy::new("physics")).1,
        ];
        arbitrator().allocate(
            &playing(),
            &frame_of(7.0),
            Trigger::Refresh,
            &agents,
            &measured,
            &nothing(),
        )
    };
    assert_eq!(
        round(nothing()).trigger,
        Trigger::Refresh,
        "2 + 2 ms fit 7 ms"
    );
    let calibrated = round([(AgentId::new("renderer"), 24.0)].into());
    assert_eq!(calibrated.trigger, Trigger::Safety, "6 + 2 ms exceed 7 ms");
}

/// **The calibration factor is clamped to ×4.** A pathological 800 ms
/// against an 8 ms quote is ×100 unclamped (a 200 ms floor); clamped, the
/// floor is 8 ms, and 8 + 2 ms fit 12 ms. The envelope holds the calibrated
/// floor.
#[test]
fn the_calibration_factor_is_clamped() {
    let agents = [
        shared(Legacy::new("renderer")).1,
        shared(Legacy::new("physics")).1,
    ];
    let allocation = arbitrator().allocate(
        &playing(),
        &frame_of(12.0),
        Trigger::Refresh,
        &agents,
        &[(AgentId::new("renderer"), 800.0)].into(),
        &nothing(),
    );
    assert_eq!(allocation.trigger, Trigger::Refresh, "{allocation:?}");
    assert!(lent_to(&allocation, "renderer").time.cpu >= ms(8));
}

/// **A round keeps the offers as the agents answered them, for its epoch
/// only.** `renderer` quotes `LowPower` at 2 ms and was measured at 24 ms
/// against its 8 ms `Balanced`: the round decides on rows scaled ×3, but the
/// offer it keeps for the journal holds the 2 ms quoted, for the round's
/// epoch. The next round, with no agent, keeps no offer of the last one.
#[test]
fn a_round_keeps_the_offers_as_answered_for_its_epoch() {
    let mut arbitrator = arbitrator();
    let agents = [shared(Legacy::new("renderer")).1];
    let allocation = arbitrator.allocate(
        &playing(),
        &frame_of(40.0),
        Trigger::Refresh,
        &agents,
        &[(AgentId::new("renderer"), 24.0)].into(),
        &nothing(),
    );

    let offers = arbitrator.offers();
    assert_eq!(offers.len(), 1, "{offers:?}");
    assert_eq!(offers[0].agent, AgentId::new("renderer"));
    assert_eq!(offers[0].epoch, allocation.epoch, "the round's epoch");
    let floor = (offers[0].rows.iter())
        .find(|row| row.id == LowPower)
        .expect("the floor is offered");
    assert_eq!(floor.cost.time.cpu, ms(2), "as quoted, not calibrated");

    allocate(&mut arbitrator, &playing(), 40.0, Trigger::Refresh, &[]);
    assert!(
        arbitrator.offers().is_empty(),
        "a round keeps only its own offers: {:?}",
        arbitrator.offers()
    );
}

// ── Policies ──────────────────────────────────────────────────────

/// **A `Manual` agent is pinned against the budget.** The bridge applies the
/// pinned `LowPower` though the frame would hold `HighPerformance`, and the
/// allocation names the pin.
#[test]
fn a_manual_mode_pins_the_row_against_the_budget() {
    let (legacy, agent) = shared(Legacy::new("renderer"));
    let mut arbitrator = arbitrator();
    arbitrator.set_adaptation_mode(AgentId::new("renderer"), AdaptationMode::Manual(LowPower));
    let agents = [agent];
    let allocation = allocate(&mut arbitrator, &playing(), 20.0, Trigger::Initial, &agents);
    lend(&allocation, &agents);
    assert_eq!(allocation.pins, vec![(AgentId::new("renderer"), LowPower)]);
    assert_eq!(lock(&legacy).applied().strategy_id, LowPower);
}

/// **A `Bounded` agent runs within its range.** The envelope would hold
/// `HighPerformance`; the allocation allows `LowPower..=Balanced` only, so
/// the bridge applies `Balanced`.
#[test]
fn a_bounded_agent_runs_within_its_range() {
    let (legacy, agent) = shared(Legacy::new("renderer"));
    let mut arbitrator = arbitrator();
    arbitrator.set_adaptation_mode(
        AgentId::new("renderer"),
        AdaptationMode::Bounded {
            min: LowPower,
            max: Balanced,
        },
    );
    let agents = [agent];
    let allocation = allocate(&mut arbitrator, &playing(), 20.0, Trigger::Initial, &agents);
    lend(&allocation, &agents);
    let allowed = allocation
        .allowed
        .iter()
        .find(|(id, _)| *id == AgentId::new("renderer"))
        .map(|(_, rows)| rows.clone())
        .expect("the range is in the allocation");
    assert!(allowed.contains(&LowPower) && allowed.contains(&Balanced));
    assert!(!allowed.contains(&HighPerformance), "{allowed:?}");
    assert_eq!(lock(&legacy).applied().strategy_id, Balanced);
}

/// **A `Stable` agent is not upgraded on recovery.** Its envelope from a
/// 10 ms frame is kept when the frame recovers to 20 ms, and shrinks when
/// the frame falls to 6 ms.
#[test]
fn a_stable_agent_is_not_upgraded_on_recovery() {
    let (_bidder, agent) = shared(Bidder::single("renderer", 2_000));
    let mut arbitrator = arbitrator();
    arbitrator.set_adaptation_mode(AgentId::new("renderer"), AdaptationMode::Stable);
    let agents = [agent];

    let held = lent_to(
        &allocate(&mut arbitrator, &playing(), 10.0, Trigger::Initial, &agents),
        "renderer",
    );
    let recovered = lent_to(
        &allocate(
            &mut arbitrator,
            &playing(),
            20.0,
            Trigger::FrameGovernor,
            &agents,
        ),
        "renderer",
    );
    assert_eq!(recovered, held, "no opportunistic upgrade");
    let pressed = lent_to(
        &allocate(&mut arbitrator, &playing(), 6.0, Trigger::Pressure, &agents),
        "renderer",
    );
    assert!(pressed.time.cpu < held.time.cpu, "{pressed:?} vs {held:?}");
    assert!(pressed.time.cpu <= ms(6));
}

/// **A `Cap` hint is a ceiling on the envelope.** At 20 ms the bridge would
/// apply `HighPerformance` (14 ms); a 10 ms cap leaves `Balanced` (8 ms), a
/// 5 ms cap leaves `LowPower` (2 ms), and the envelope never passes the cap.
#[test]
fn a_cap_hint_bounds_the_envelope() {
    for (cap_ms, expected) in [(10.0, Balanced), (5.0, LowPower)] {
        let (legacy, agent) = shared(Legacy::new("renderer"));
        let hints: HashMap<AgentId, AgentHints> = [(
            AgentId::new("renderer"),
            AgentHints {
                cap_ms: Some(cap_ms),
                priority: None,
            },
        )]
        .into();
        let agents = [agent];
        let allocation = arbitrator().allocate(
            &playing(),
            &frame_of(20.0),
            Trigger::Refresh,
            &agents,
            &nothing(),
            &hints,
        );
        lend(&allocation, &agents);
        let lent = lent_to(&allocation, "renderer");
        assert!(
            busiest(&lent) <= Duration::from_secs_f32(cap_ms / 1000.0) + us(1),
            "cap {cap_ms} ms: {lent:?}"
        );
        assert_eq!(
            lock(&legacy).applied().strategy_id,
            expected,
            "cap {cap_ms} ms"
        );
    }
}

/// **A hint reaches only its agent.** A 1 ms cap on `audio` leaves
/// `renderer`'s envelope as it is without any hint.
#[test]
fn a_cap_on_another_agent_changes_nothing() {
    let round = |hints: &HashMap<AgentId, AgentHints>| {
        let (_, agent) = shared(Legacy::new("renderer"));
        arbitrator().allocate(
            &playing(),
            &frame_of(20.0),
            Trigger::Refresh,
            &[agent],
            &nothing(),
            hints,
        )
    };
    let elsewhere: HashMap<AgentId, AgentHints> = [(
        AgentId::new("audio"),
        AgentHints {
            cap_ms: Some(1.0),
            priority: None,
        },
    )]
    .into();
    assert_eq!(round(&elsewhere).envelopes, round(&nothing()).envelopes);
}

/// **The VRAM the context knows bounds the envelopes.** At 20 ms the bridge
/// would apply `HighPerformance`, but its 20 MiB do not fit 15 MiB: it
/// applies `Balanced` (10 MiB). Without a VRAM figure nothing limits it.
#[test]
fn vram_known_to_the_context_bounds_the_envelopes() {
    let lent_with = |vram: Option<u64>| {
        let (legacy, agent) = shared(Legacy::new("renderer"));
        let mut context = playing();
        context.hardware.available_vram = vram;
        let agents = [agent];
        let allocation = allocate(&mut arbitrator(), &context, 20.0, Trigger::Refresh, &agents);
        lend(&allocation, &agents);
        if let Some(vram) = vram {
            assert!(lent_to(&allocation, "renderer").memory.gpu <= vram);
        }
        legacy
    };
    assert_eq!(
        lock(&lent_with(Some(15 * MIB))).applied().strategy_id,
        Balanced,
        "the VRAM is the binding limit"
    );
    assert_eq!(
        lock(&lent_with(None)).applied().strategy_id,
        HighPerformance,
        "nothing limits the memory"
    );
}

// ── Late offers ───────────────────────────────────────────────────

/// **I10: an offer after `reply_by` keeps the agent's envelope, and the
/// allocation names it.** `zinc` answers on time in a 10 ms frame, then late
/// when the frame grows to 20 ms: it keeps its 10 ms-frame envelope while
/// `ash`, on time, is lent more.
#[test]
fn a_late_offer_keeps_the_envelope() {
    let (ash, first) = shared(Bidder::single("ash", 2_000));
    let (zinc, second) = shared(Bidder::single("zinc", 2_000));
    // The late agent is asked last, so its lateness cannot delay the other.
    let agents = [first, second];
    let mut arbitrator = arbitrator();

    let on_time = allocate(&mut arbitrator, &playing(), 10.0, Trigger::Initial, &agents);
    assert!(on_time.late.is_empty(), "{:?}", on_time.late);
    let held = lent_to(&on_time, "zinc");

    lock(&zinc).late = true;
    let after = allocate(
        &mut arbitrator,
        &playing(),
        20.0,
        Trigger::FrameGovernor,
        &agents,
    );
    assert_eq!(after.late, vec![AgentId::new("zinc")]);
    assert_eq!(lent_to(&after, "zinc"), held, "the envelope it had");
    assert!(
        lent_to(&after, "ash").time.cpu > lent_to(&on_time, "ash").time.cpu,
        "the agent on time is lent the larger frame"
    );
    assert_eq!(
        lock(&ash).asked.last().map(|request| request.epoch),
        Some(after.epoch),
        "the agent on time was asked for the new epoch"
    );
}

// ── The emergency ─────────────────────────────────────────────────

/// **The emergency gives every agent its floor**, and names no row: a death
/// spiral makes the round `Safety`. `grid` speaks the protocol and has no
/// `LowPower` (its floor is `Custom(1)`); `skybox` only negotiates and has no
/// `LowPower` either (its floor is `Balanced`). Each is lent exactly its
/// floor, and the bridge applies `skybox`'s floor.
#[test]
fn the_emergency_gives_every_agent_its_floor() {
    let (grid, first) = shared(Bidder::new(
        "grid",
        vec![row(Custom(2), 3_000, 1.0), row(Custom(1), 1_000, 0.4)],
        Custom(1),
    ));
    let (skybox, second) = shared(Legacy::with_options(
        "skybox",
        vec![
            option(Balanced, ms(3), 2 * MIB),
            option(HighPerformance, ms(9), 4 * MIB),
        ],
    ));
    let mut spiral = frame_of(16.66);
    spiral.death_spiral_detected = true;

    let agents = [first, second];
    let allocation = arbitrator().allocate(
        &playing(),
        &spiral,
        Trigger::Pressure,
        &agents,
        &nothing(),
        &nothing(),
    );
    lend(&allocation, &agents);

    assert_eq!(allocation.trigger, Trigger::Safety);
    assert!(allocation.pins.is_empty() && allocation.allowed.is_empty());
    let grid = lock(&grid);
    assert_eq!(
        lent_to(&allocation, "grid"),
        envelope_of(&grid.floor_cost())
    );
    assert_eq!(grid.last_lent().envelope, envelope_of(&grid.floor_cost()));
    let skybox_lent = lent_to(&allocation, "skybox");
    assert_eq!(skybox_lent.time.cpu, ms(3), "{skybox_lent:?}");
    assert_eq!(skybox_lent.memory.gpu, 2 * MIB);
    assert_eq!(lock(&skybox).applied().strategy_id, Balanced);
}

/// **Two stalled agents make the round `Safety`**: both are lent their
/// floor, which the bridge applies.
#[test]
fn two_stalled_agents_make_the_round_safety() {
    let (renderer, first) = shared(Legacy::stalled("renderer"));
    let (physics, second) = shared(Legacy::stalled("physics"));
    let agents = [first, second];
    let allocation = allocate(
        &mut arbitrator(),
        &playing(),
        16.66,
        Trigger::Refresh,
        &agents,
    );
    lend(&allocation, &agents);
    assert_eq!(allocation.trigger, Trigger::Safety);
    assert_eq!(lock(&renderer).applied().strategy_id, LowPower);
    assert_eq!(lock(&physics).applied().strategy_id, LowPower);
}

// ── Kept envelopes across live rounds ─────────────────────────────

/// **`Stable` agents keep their envelopes on a machine that says nothing of
/// its memory.** The first round lends envelopes of unknown memory; the
/// next round, with both agents `Stable`, decides from them and lends each
/// the one it held.
#[test]
fn stable_agents_keep_their_envelopes_when_the_memory_is_unknown() {
    let (_ash, first) = shared(Bidder::single("ash", 2_000));
    let (_birch, second) = shared(Bidder::single("birch", 3_000));
    let agents = [first, second];
    let mut arbitrator = arbitrator();
    arbitrator.set_adaptation_mode(AgentId::new("ash"), AdaptationMode::Stable);
    arbitrator.set_adaptation_mode(AgentId::new("birch"), AdaptationMode::Stable);

    let held = allocate(&mut arbitrator, &playing(), 10.0, Trigger::Initial, &agents);
    let kept = allocate(
        &mut arbitrator,
        &playing(),
        20.0,
        Trigger::FrameGovernor,
        &agents,
    );
    assert_eq!(lent_to(&kept, "ash"), lent_to(&held, "ash"));
    assert_eq!(lent_to(&kept, "birch"), lent_to(&held, "birch"));
}

/// **A late agent does not make the round `Safety` once the VRAM is
/// known.** The first round, before the machine names its VRAM, lends
/// envelopes of unknown memory. When the VRAM is known and `zinc` answers
/// late, it keeps its envelope; the floors (2 + 2 ms, no graphics memory)
/// still fit, so the round keeps its trigger.
#[test]
fn a_late_agent_does_not_make_the_round_safety_once_vram_is_known() {
    let (_ash, first) = shared(Bidder::single("ash", 2_000));
    let (zinc, second) = shared(Bidder::single("zinc", 2_000));
    let agents = [first, second];
    let mut arbitrator = arbitrator();

    allocate(&mut arbitrator, &playing(), 10.0, Trigger::Initial, &agents);
    lock(&zinc).late = true;
    let mut known = playing();
    known.hardware.available_vram = Some(512 * MIB);
    let after = allocate(
        &mut arbitrator,
        &known,
        10.0,
        Trigger::FrameGovernor,
        &agents,
    );
    assert_eq!(after.trigger, Trigger::FrameGovernor, "{after:?}");
    assert_eq!(after.late, vec![AgentId::new("zinc")]);
}

/// **An agent too busy to be asked keeps its envelope.** `ash` holds its
/// lock through the second round, so it is neither asked nor lent: it keeps
/// running in the envelope it holds. The allocation still names that
/// envelope (as a late agent's) so `birch` is not lent `ash`'s time, and the
/// round after can keep it again.
#[test]
fn an_agent_busy_through_a_round_keeps_its_envelope() {
    let (ash, first) = shared(Bidder::single("ash", 2_000));
    let (_birch, second) = shared(Bidder::single("birch", 2_000));
    let agents = [first, second];
    let mut arbitrator = GornaArbitrator::new(Duration::from_millis(5));

    let before = allocate(&mut arbitrator, &playing(), 10.0, Trigger::Initial, &agents);
    let held = lent_to(&before, "ash");

    let (release, hold) = std::sync::mpsc::channel::<()>();
    let (holding, locked) = std::sync::mpsc::channel::<()>();
    let busy = {
        let ash = Arc::clone(&ash);
        std::thread::spawn(move || {
            let _running = ash.lock().expect("the agent lock");
            holding.send(()).expect("the test waits");
            let _ = hold.recv();
        })
    };
    locked.recv().expect("the agent is busy");
    let during = allocate(
        &mut arbitrator,
        &playing(),
        20.0,
        Trigger::FrameGovernor,
        &agents,
    );
    drop(release);
    busy.join().expect("the busy thread");

    let named = during
        .envelopes
        .iter()
        .find(|(agent, _)| *agent == AgentId::new("ash"))
        .map(|(_, envelope)| *envelope);
    assert_eq!(named, Some(held), "{during:?}");
    assert!(
        lent_to(&during, "birch").time.cpu + held.time.cpu <= ms(20),
        "birch is not lent the time ash runs in: {during:?}"
    );
}

/// Runs `round` while another thread holds `agent`'s lock.
fn while_busy<A: Send + 'static, R>(agent: &Arc<Mutex<A>>, round: impl FnOnce() -> R) -> R {
    let (release, hold) = std::sync::mpsc::channel::<()>();
    let (holding, locked) = std::sync::mpsc::channel::<()>();
    let busy = {
        let agent = Arc::clone(agent);
        std::thread::spawn(move || {
            let _running = agent.lock().expect("the agent lock");
            holding.send(()).expect("the test waits");
            let _ = hold.recv();
        })
    };
    locked.recv().expect("the agent is busy");
    let result = round();
    drop(release);
    busy.join().expect("the busy thread");
    result
}

/// **An agent busy through a round keeps its memory too.** The VRAM is
/// known (512 MiB) and `ash` holds 10 MiB of it; busy through the second
/// round, it keeps the whole envelope it held, graphics memory included, and
/// `birch` is not lent the memory `ash` holds.
#[test]
fn an_agent_busy_through_a_round_keeps_its_memory() {
    let (ash, first) = shared(Bidder::new(
        "ash",
        vec![row_holding(LowPower, 2_000, 10 * MIB, 0.4)],
        LowPower,
    ));
    let (_birch, second) = shared(Bidder::new(
        "birch",
        vec![row_holding(LowPower, 2_000, 10 * MIB, 0.4)],
        LowPower,
    ));
    let agents = [first, second];
    let mut arbitrator = GornaArbitrator::new(Duration::from_millis(5));
    let mut known = playing();
    known.hardware.available_vram = Some(512 * MIB);

    let before = allocate(&mut arbitrator, &known, 10.0, Trigger::Initial, &agents);
    let held = lent_to(&before, "ash");
    let during = while_busy(&ash, || {
        allocate(
            &mut arbitrator,
            &known,
            10.0,
            Trigger::FrameGovernor,
            &agents,
        )
    });

    assert_eq!(lent_to(&during, "ash"), held, "{during:?}");
    assert!(
        lent_to(&during, "birch").memory.gpu + held.memory.gpu <= 512 * MIB,
        "{during:?}"
    );
}

/// **A late agent keeps its memory while it still fits.** In a 100 MiB
/// device, `zinc` is lent 42 MiB in the first round. In the second it is
/// late while `ash` now holds 30 MiB: 42 + 30 MiB still fit, so `zinc` keeps
/// its 42 MiB, and the two envelopes stay within the device.
#[test]
fn a_late_agent_keeps_its_memory_while_it_fits() {
    let (ash, first) = shared(Bidder::new(
        "ash",
        vec![row_holding(LowPower, 2_000, 10 * MIB, 0.4)],
        LowPower,
    ));
    let (zinc, second) = shared(Bidder::new(
        "zinc",
        vec![row_holding(LowPower, 2_000, 10 * MIB, 0.4)],
        LowPower,
    ));
    let agents = [first, second];
    let mut arbitrator = arbitrator();
    let mut known = playing();
    known.hardware.available_vram = Some(100 * MIB);

    let before = allocate(&mut arbitrator, &known, 10.0, Trigger::Initial, &agents);
    let held = lent_to(&before, "zinc");
    lock(&ash).rows = vec![row_holding(LowPower, 2_000, 30 * MIB, 0.4)];
    lock(&zinc).late = true;
    let after = allocate(
        &mut arbitrator,
        &known,
        10.0,
        Trigger::FrameGovernor,
        &agents,
    );

    assert_eq!(after.late, vec![AgentId::new("zinc")]);
    assert_eq!(
        lent_to(&after, "zinc").memory.gpu,
        held.memory.gpu,
        "{after:?}"
    );
    assert!(
        lent_to(&after, "ash").memory.gpu + held.memory.gpu <= 100 * MIB,
        "{after:?}"
    );
}
