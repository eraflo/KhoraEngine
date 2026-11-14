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

//! How the running DCC drives its frame governor: the setpoint it decides
//! against, the epoch a decision opens, and the lever GORNA's allocation in
//! force leaves it.

use super::*;

/// Sends `count` frames of `frame_ms` at once.
fn frames(tx: &crossbeam_channel::Sender<TelemetryEvent>, count: usize, frame_ms: f64) {
    for _ in 0..count {
        tx.send(TelemetryEvent::FrameReport(frame_report(frame_ms, None)))
            .expect("the DCC listens");
    }
}

/// Sends three frames of `frame_ms` every 10 ms for a second; the lowest
/// multiplier seen meanwhile.
fn lowest_multiplier_over_a_second(
    dcc: &DccService,
    tx: &crossbeam_channel::Sender<TelemetryEvent>,
    frame_ms: f64,
) -> f32 {
    let mut lowest = dcc.get_context().global_budget_multiplier;
    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    while std::time::Instant::now() < deadline {
        frames(tx, 3, frame_ms);
        thread::sleep(Duration::from_millis(10));
        lowest = lowest.min(dcc.get_context().global_budget_multiplier);
    }
    lowest
}

/// **The governor's setpoint ignores the forecast.**
///
/// A streamer's cost model forecasts 24 ms, over the 16.66 ms frame: every tick
/// the forecast tightens the target GORNA shares to about 11.6 ms, and epochs
/// open for the breach. The streamer reports runs but is not one of the agents
/// GORNA lends to, so its measured cost does not scale the renderer's quotes:
/// the renderer's upgrade still fits, and the governor has a lever. Frames of
/// 14 ms run under the governor's own target (the untightened setpoint less its
/// headroom, 15 ms): the multiplier stays at 1.0. Decided against the tightened
/// setpoint, they would pull it down — the forecast's cut would count twice.
/// The governor is awake all the same: frames of 17 ms, over its target, pull
/// the multiplier down.
#[test]
fn the_governors_setpoint_ignores_the_forecast() {
    let (mut dcc, rx) = recording(DccConfig {
        min_epoch_ticks: 1,
        ..Default::default()
    });
    let (agent, _ear) = Speaker::with_upgrade(
        "renderer",
        Duration::from_millis(2),
        Duration::from_millis(8),
    );
    dcc.register_agent(agent, 1.0);
    let tx = dcc.event_sender();
    for n in [10.0, 20.0, 40.0, 80.0] {
        tx.send(TelemetryEvent::AgentReport(run_report(
            "streamer",
            Epoch(0),
            (300.0 * n) as u64,
            n,
        )))
        .expect("the DCC listens");
    }
    dcc.start(rx);

    let breached = wait_for(3, || opened_by(&dcc, Trigger::ForecastBreach));
    let lowest = lowest_multiplier_over_a_second(&dcc, &tx, 14.0);
    let awake = wait_for(3, || {
        frames(&tx, 3, 17.0);
        dcc.get_context().global_budget_multiplier < 1.0
    });
    dcc.stop();

    assert!(breached, "the forecast breaches the frame");
    assert!(
        lowest >= 1.0,
        "14 ms frames are under the governor's target whatever the forecast, lowest {lowest}"
    );
    assert!(awake, "17 ms frames pull the multiplier down");
}

/// **A governor decision opens an epoch of its own, however small.**
///
/// Frames of 15.5 ms run just over the governor's 15 ms target: it lowers the
/// multiplier to about 0.967, and an epoch opens, by `Trigger::FrameGovernor`.
/// No other trigger is raised: the frames stay under the heuristics' warning,
/// steady, and no cost model forecasts anything.
#[test]
fn a_governor_decision_opens_an_epoch_of_its_own_however_small() {
    let (mut dcc, rx) = recording(DccConfig::default());
    let (agent, _ear) = Speaker::with_upgrade(
        "renderer",
        Duration::from_millis(2),
        Duration::from_millis(8),
    );
    dcc.register_agent(agent, 1.0);
    let tx = dcc.event_sender();
    dcc.start(rx);

    let initial = wait_for(2, || !epochs(&dcc).is_empty());
    frames(&tx, 2 * FRAME_TIME_MIN_SAMPLES, 15.5);
    let opened = wait_for(3, || opened_by(&dcc, Trigger::FrameGovernor));
    let multiplier = dcc.get_context().global_budget_multiplier;
    let issued = epochs(&dcc);
    dcc.stop();

    assert!(initial, "the first epoch opens");
    assert!(
        opened,
        "the governor's decision opens an epoch: {:?}",
        issued.iter().map(|e| e.trigger).collect::<Vec<_>>()
    );
    assert!(
        multiplier < 1.0 && multiplier > 0.95,
        "the decision is a small one: {multiplier}"
    );
}

/// **With every agent on its floor, the multiplier holds.**
///
/// The only agent offers its floor alone: the allocation in force takes no
/// upgrade, it has no price — a lower target would change nothing. Frames of 17
/// ms, over the governor's target, leave the multiplier at 1.0. The same frames
/// with an agent that can give up an upgrade pull it down.
#[test]
fn with_every_agent_on_its_floor_the_multiplier_holds() {
    let (mut dcc, rx) = recording(DccConfig::default());
    let (agent, _ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    dcc.register_agent(agent, 1.0);
    let tx = dcc.event_sender();
    dcc.start(rx);
    let initial = wait_for(2, || !epochs(&dcc).is_empty());
    let unpriced = epochs(&dcc).first().is_some_and(|e| e.price.is_none());
    let lowest = lowest_multiplier_over_a_second(&dcc, &tx, 17.0);
    dcc.stop();

    let (mut levered, rx) = recording(DccConfig::default());
    let (agent, _ear) = Speaker::with_upgrade(
        "renderer",
        Duration::from_millis(2),
        Duration::from_millis(8),
    );
    levered.register_agent(agent, 1.0);
    let tx = levered.event_sender();
    levered.start(rx);
    let priced = wait_for(2, || {
        epochs(&levered).first().is_some_and(|e| e.price.is_some())
    });
    let lowered = wait_for(3, || {
        frames(&tx, 3, 17.0);
        levered.get_context().global_budget_multiplier < 1.0
    });
    levered.stop();

    assert!(initial && unpriced, "the floor-only epoch takes no upgrade");
    assert!(
        lowest >= 1.0,
        "with every agent on its floor, 17 ms frames leave the multiplier at 1.0, lowest {lowest}"
    );
    assert!(priced, "the agent with an upgrade takes it");
    assert!(
        lowered,
        "with an upgrade to give up, 17 ms frames pull the multiplier down"
    );
}

/// **The governor moves again only once an epoch lent its last move.**
///
/// An epoch lasts three ticks at least (`min_epoch_ticks`), the governor
/// decides once a tick. Frames of 30 ms, over the target whatever the
/// multiplier — three a tick, enough to settle — pull the multiplier down from
/// 1.0 step by step. A step decided on frames that ran before the last one was
/// lent measures nothing of it: each multiplier the DCC publishes is the target
/// of an epoch (16.66 ms × the multiplier) before the next one replaces it.
#[test]
fn the_governor_moves_again_only_once_an_epoch_lent_its_last_move() {
    let (mut dcc, rx) = recording(DccConfig::default());
    let (agent, _ear) = Speaker::with_upgrade(
        "renderer",
        Duration::from_millis(2),
        Duration::from_millis(8),
    );
    dcc.register_agent(agent, 1.0);
    let tx = dcc.event_sender();
    dcc.start(rx);
    let initial = wait_for(2, || !epochs(&dcc).is_empty());

    // Every multiplier published, in order.
    let mut published = vec![dcc.get_context().global_budget_multiplier];
    let deadline = std::time::Instant::now() + Duration::from_millis(800);
    while std::time::Instant::now() < deadline {
        frames(&tx, 3, 30.0);
        for _ in 0..10 {
            thread::sleep(Duration::from_millis(1));
            let multiplier = dcc.get_context().global_budget_multiplier;
            if published
                .last()
                .is_none_or(|last| (last - multiplier).abs() > 1e-4)
            {
                published.push(multiplier);
            }
        }
    }
    let lent: Vec<f64> = records(&dcc)
        .iter()
        .map(|record| record.target.as_secs_f64() * 1_000.0)
        .collect();
    dcc.stop();

    assert!(initial, "the first epoch opens");
    assert!(
        published.len() >= 3,
        "30 ms frames pull the multiplier down step by step: {published:?}"
    );
    let unlent: Vec<f32> = published[..published.len() - 1]
        .iter()
        .copied()
        .filter(|m| {
            !lent
                .iter()
                .any(|target| (target - 16.66 * f64::from(*m)).abs() < 0.01)
        })
        .collect();
    assert!(
        unlent.is_empty(),
        "the governor moved on from {unlent:?} before an epoch lent it: published {published:?}, epochs' targets {lent:?} ms"
    );
}

/// **A `Stable` agent above its floor leaves the governor its lever.**
///
/// The renderer, `Stable`, took its 8 ms upgrade in the first epoch; an audio
/// agent offering its floor alone joins, and the epoch it opens keeps the
/// renderer's envelope — a `Stable` agent keeps what it holds while it fits —
/// and takes no upgrade: it has no price. Not every agent is on its floor: a
/// lower target is one the renderer's envelope no longer fits, and it gives
/// way. Frames of 17 ms, over the governor's target, pull the multiplier down.
#[test]
fn a_stable_agent_above_its_floor_leaves_the_governor_its_lever() {
    use khora_core::agent::gorna::AdaptationMode;

    let floor = Duration::from_millis(2);
    let (mut dcc, rx) = recording(DccConfig::default());
    let (renderer, _renderer_ear) =
        Speaker::with_upgrade("renderer", floor, Duration::from_millis(8));
    dcc.register_agent(renderer, 1.0);
    dcc.set_adaptation_mode(AgentId::new("renderer"), AdaptationMode::Stable);
    let tx = dcc.event_sender();
    dcc.start(rx);
    let upgraded = wait_for(2, || {
        epochs(&dcc).first().is_some_and(|e| e.price.is_some())
    });

    let (audio, _audio_ear) = Speaker::with_ear("audio", Duration::from_millis(1));
    dcc.register_agent(audio, 1.0);
    let joined = wait_for(3, || opened_by(&dcc, Trigger::AgentsChanged));
    let kept = epochs(&dcc)
        .into_iter()
        .find(|e| e.trigger == Trigger::AgentsChanged);
    let lowered = wait_for(3, || {
        frames(&tx, 3, 17.0);
        dcc.get_context().global_budget_multiplier < 1.0
    });
    dcc.stop();

    assert!(
        upgraded,
        "the renderer takes its upgrade in the first epoch"
    );
    assert!(joined, "the audio agent's arrival opens an epoch");
    let kept = kept.expect("the epoch the audio agent opened");
    let renderer_envelope = kept
        .envelopes
        .iter()
        .find(|(id, _)| *id == AgentId::new("renderer"))
        .map(|(_, envelope)| envelope.time.cpu);
    assert!(
        kept.price.is_none() && renderer_envelope.is_some_and(|lent| lent > floor),
        "the renderer keeps an envelope above its floor, unpriced: {kept:?}"
    );
    assert!(
        lowered,
        "with the renderer above its floor, 17 ms frames pull the multiplier down"
    );
}

/// **A `Stable` agent on its floor leaves the multiplier where it is.**
///
/// The only agent, `Stable`, offers its floor alone: every epoch lends it that
/// floor and the slack, and the ones after the first keep that envelope — it
/// still has nothing but its floor to run. Frames of 20 ms, over the governor's
/// target (and the heuristics' warning, so epochs keep opening), leave the
/// multiplier at 1.0, as they do for a `Learning` agent on its floor: a lower
/// target would take back slack, never a row.
#[test]
fn a_stable_agent_on_its_floor_leaves_the_multiplier_where_it_is() {
    use khora_core::agent::gorna::AdaptationMode;

    let (mut dcc, rx) = recording(DccConfig::default());
    let (agent, _ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    dcc.register_agent(agent, 1.0);
    dcc.set_adaptation_mode(AgentId::new("renderer"), AdaptationMode::Stable);
    let tx = dcc.event_sender();
    dcc.start(rx);
    let initial = wait_for(2, || !epochs(&dcc).is_empty());
    let lowest = lowest_multiplier_over_a_second(&dcc, &tx, 20.0);
    let issued = epochs(&dcc);
    dcc.stop();

    assert!(initial, "the first epoch opens");
    assert!(
        lowest >= 1.0,
        "with its agent on its floor, 20 ms frames leave the multiplier at 1.0, lowest {lowest}; epochs {:?}",
        issued
            .iter()
            .map(|e| (e.trigger, e.lever))
            .collect::<Vec<_>>()
    );
}

/// **The governor does not move again on frames under a replayed allocation.**
///
/// A replay of twenty recorded epochs, one a tick, each leaving a lever: their
/// envelopes were decided when they were recorded, whatever the multiplier is
/// now. Frames of 30 ms, over the target, pull the multiplier down from 1.0 to
/// 0.7 — and no frame of the replay runs under that change: the replayed epochs
/// do not carry it. The governor waits for an epoch that lends its move before
/// it moves again, so the multiplier never goes under 0.7 while the replay
/// lasts.
#[test]
fn the_governor_does_not_move_again_on_frames_under_a_replayed_allocation() {
    let (mut dcc, rx) = DccService::new(DccConfig {
        tick_rate: 20,
        ..Default::default()
    });
    let (agent, _ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    dcc.register_agent(agent, 1.0);
    let replayed: Vec<EpochRecord> = (1..=20)
        .map(|epoch| {
            let mut recorded = allocation(
                epoch,
                Trigger::Refresh,
                vec![(AgentId::new("renderer"), cpu_envelope(8_000))],
            );
            recorded.lever = true;
            record(recorded)
        })
        .collect();
    dcc.replay_decisions(DecisionTrace { epochs: replayed });
    let tx = dcc.event_sender();
    dcc.start(rx);

    let mut lowest = dcc.get_context().global_budget_multiplier;
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while dcc.is_replaying() && std::time::Instant::now() < deadline {
        frames(&tx, 3, 30.0);
        thread::sleep(Duration::from_millis(5));
        lowest = lowest.min(dcc.get_context().global_budget_multiplier);
    }
    let replay_over = !dcc.is_replaying();
    dcc.stop();

    assert!(replay_over, "the replay runs out");
    assert!(
        lowest >= 0.7 - 1e-3,
        "the multiplier moved again on frames no epoch lent its last move to: lowest {lowest}"
    );
}

/// **The DCC plans its rounds on the frames its governor keeps.**
///
/// Five frames of 10 ms, then seven of 100 to 700 ms, queued before the DCC
/// starts: its first tick reads them all, then opens its first epoch. The
/// governor leaves the 100 ms frame out — a hitch after frames within the
/// setpoint — and keeps the six after it as load, bounded at `hitch_ratio ×
/// setpoint`: eleven samples, five of 10 ms and six of 33.3 ms, whose median
/// absolute deviation is zero. The first epoch is planned with no margin.
/// Planned on the raw frames, their spread would keep some 340 ms free of a
/// 16.7 ms target.
#[test]
fn the_dcc_plans_its_rounds_on_the_frames_its_governor_keeps() {
    let (mut dcc, rx) = recording(DccConfig::default());
    let (agent, _ear) = Speaker::with_upgrade(
        "renderer",
        Duration::from_millis(2),
        Duration::from_millis(8),
    );
    dcc.register_agent(agent, 1.0);
    let tx = dcc.event_sender();
    frames(&tx, 5, 10.0);
    for load_ms in [100.0, 200.0, 300.0, 400.0, 500.0, 600.0, 700.0] {
        frames(&tx, 1, load_ms);
    }
    dcc.start(rx);

    let opened = wait_for(3, || !records(&dcc).is_empty());
    let recorded = records(&dcc);
    dcc.stop();

    assert!(opened, "the first epoch opens");
    let first = recorded.first().expect("the first epoch");
    assert_eq!(
        first.frames.count, 12,
        "the first epoch is decided after the tick read the twelve frames"
    );
    assert_eq!(
        first.planning.margin,
        Duration::ZERO,
        "the first epoch is planned on the governor's samples, not the raw frames: {:?}",
        first.planning
    );
}
