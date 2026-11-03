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

//! What the running DCC says to its agents: offers asked, allocations
//! published for the scheduler to lend, policies, the governor's effect on the
//! envelopes, replay, the journal — and only to the agents active in the
//! engine's mode.

use super::*;

/// The envelope the latest epoch lends `name`, if one does.
fn last_lent_to(dcc: &DccService, name: &'static str) -> Option<Envelope> {
    epochs(dcc).iter().rev().find_map(|epoch| {
        epoch
            .envelopes
            .iter()
            .find(|(id, _)| *id == AgentId::new(name))
            .map(|(_, envelope)| *envelope)
    })
}

/// Whether some epoch lends `name` an envelope.
fn names(dcc: &DccService, name: &'static str) -> bool {
    last_lent_to(dcc, name).is_some()
}

/// A DCC ticking at 100 Hz, with a scheduler to be started beside it.
fn host() -> Host {
    Host::new(DccConfig {
        tick_rate: 100,
        ..Default::default()
    })
}

/// **The first epoch lends every agent an envelope**, through the protocol:
/// the agent is asked for an offer and, at a frame start, applied an
/// envelope — never negotiated with nor applied a budget. The first
/// allocation is `Initial`.
#[test]
fn the_first_epoch_lends_every_agent_an_envelope() {
    let mut host = host();
    let (agent, ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    host.dcc.register_agent(agent, 1.0);
    host.start();

    let lent = host.frames_until(3, |_| !ear.heard().lent.is_empty());
    let first = epochs(&host.dcc).first().cloned();
    host.dcc.stop();

    assert!(lent, "the agent was lent an envelope");
    let heard = ear.heard();
    assert!(!heard.asked.is_empty(), "the agent was asked for an offer");
    assert_eq!(heard.old_calls, 0, "never negotiated with nor budgeted");
    let first = first.expect("the epoch was recorded");
    assert_eq!(first.trigger, Trigger::Initial);
    assert_eq!(heard.lent[0].0, first.epoch, "lent for the epoch recorded");
}

/// **The DCC thread never calls `apply`.** Running alone, the DCC opens its
/// epochs and lends nothing: an allocation is published, not applied. Once
/// a scheduler runs frames, the agent is lent its envelope — on the thread
/// running the frames, at a frame start, every time.
#[test]
fn the_dcc_thread_never_calls_apply() {
    let mut host = host();
    let (agent, ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    host.dcc.register_agent(agent, 1.0);
    host.start();

    let decided = wait_for(3, || !epochs(&host.dcc).is_empty());
    // A few more ticks, for a DCC that would lend late.
    thread::sleep(Duration::from_millis(50));
    let lent_by_the_dcc = ear.heard().lent.len();
    let lent = host.frames_until(3, |_| !ear.heard().lent.is_empty());
    host.dcc.stop();

    assert!(decided, "the DCC opened an epoch");
    assert_eq!(
        lent_by_the_dcc, 0,
        "an epoch opened with no frame run, and nothing was applied"
    );
    assert!(lent, "the scheduler lent the envelope");
    let here = thread::current().id();
    let heard = ear.heard();
    assert!(
        heard.lent_on.iter().all(|thread| *thread == here),
        "every apply came on the thread running the frames: {:?} vs {here:?}",
        heard.lent_on
    );
}

/// **The DCC records each epoch's allocation**: every recorded epoch lends
/// the registered agent — one that only negotiates — an envelope, which the
/// bridge applies once the scheduler lends it.
#[test]
fn the_dcc_records_each_epochs_allocation() {
    let mut host = host();
    let agent = Arc::new(Mutex::new(StubAgent { applied: None }));
    host.dcc.register_agent(agent.clone(), 1.0);
    host.start();

    let applied = host.frames_until(3, |_| {
        agent.lock().expect("the agent lock").applied.is_some()
    });
    let trace = host.dcc.stop_decision_recording();
    host.dcc.stop();

    assert!(!trace.epochs.is_empty(), "recording captured an epoch");
    assert!(
        trace.epochs.iter().all(|epoch| epoch
            .allocation
            .envelopes
            .iter()
            .any(|(id, _)| *id == AgentId::new("renderer"))),
        "every recorded epoch lends the registered agent: {:?}",
        trace.epochs
    );
    assert!(applied, "the bridge applied a budget");
}

#[test]
fn replay_runs_while_the_trace_has_epochs_left() {
    let (dcc, _rx) = DccService::new(DccConfig::default());
    assert!(!dcc.is_replaying());

    let trace = DecisionTrace {
        epochs: vec![record(allocation(1, Trigger::Initial, Vec::new()))],
    };
    dcc.replay_decisions(trace);
    assert!(dcc.is_replaying());

    dcc.stop_replay();
    assert!(!dcc.is_replaying());
}

/// **A replayed allocation reaches the agent as recorded**: the envelope it
/// lends is not one the DCC would decide (3.333 ms for a 2 ms floor in a
/// 16.66 ms frame), and the agent is lent exactly it.
#[test]
fn a_replayed_allocation_reaches_the_agent() {
    let mut host = host();
    let (agent, ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    host.dcc.register_agent(agent, 1.0);
    let recorded = cpu_envelope(3_333);
    host.dcc.replay_decisions(DecisionTrace {
        epochs: vec![record(allocation(
            42,
            Trigger::Refresh,
            vec![(AgentId::new("renderer"), recorded)],
        ))],
    });
    host.start();

    let replayed = host.frames_until(3, |_| {
        ear.heard()
            .lent
            .iter()
            .any(|(_, envelope, _)| *envelope == recorded)
    });
    host.dcc.stop();

    assert!(replayed, "lent: {:?}", ear.heard().lent);
}

/// **Replay re-issues every epoch to every agent, through the channel.** A
/// trace of three epochs, each lending the renderer and the audio their own
/// envelope, is replayed one epoch a tick: each agent is lent each epoch
/// once, in order, with its recorded envelope — at the scheduler's frame
/// starts, on the thread running the frames — and is asked for no offer.
#[test]
fn replay_reissues_every_epoch_to_every_agent() {
    // Ten ticks a second leave each replayed epoch in force for dozens of
    // frames, so the frame loop cannot step over one.
    let mut host = Host::new(DccConfig {
        tick_rate: 10,
        ..Default::default()
    });
    let (renderer, renderer_ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    let (audio, audio_ear) = Speaker::with_ear("audio", Duration::from_millis(1));
    host.dcc.register_agent(renderer, 1.0);
    host.dcc.register_agent(audio, 0.5);
    let recorded = [(41, 3_000, 1_000), (42, 4_000, 1_500), (43, 5_000, 2_000)];
    host.dcc.replay_decisions(DecisionTrace {
        epochs: recorded
            .iter()
            .map(|&(epoch, renderer_us, audio_us)| {
                record(allocation(
                    epoch,
                    Trigger::Refresh,
                    vec![
                        (AgentId::new("renderer"), cpu_envelope(renderer_us)),
                        (AgentId::new("audio"), cpu_envelope(audio_us)),
                    ],
                ))
            })
            .collect(),
    });
    host.start();

    let lent_last = |ear: &Ear| {
        ear.heard()
            .lent
            .iter()
            .any(|(epoch, _, _)| *epoch == Epoch(43))
    };
    let replayed = host.frames_until(5, |_| lent_last(&renderer_ear) && lent_last(&audio_ear));
    host.dcc.stop();

    assert!(replayed, "both agents were lent the last epoch");
    let replayed_to = |ear: &Ear| -> Vec<(Epoch, Duration)> {
        ear.heard()
            .lent
            .iter()
            .filter(|(epoch, _, _)| (41..=43).contains(&epoch.0))
            .map(|(epoch, envelope, _)| (*epoch, envelope.time.cpu))
            .collect()
    };
    let us = Duration::from_micros;
    assert_eq!(
        replayed_to(&renderer_ear),
        vec![
            (Epoch(41), us(3_000)),
            (Epoch(42), us(4_000)),
            (Epoch(43), us(5_000))
        ]
    );
    assert_eq!(
        replayed_to(&audio_ear),
        vec![
            (Epoch(41), us(1_000)),
            (Epoch(42), us(1_500)),
            (Epoch(43), us(2_000))
        ]
    );
    let here = thread::current().id();
    for ear in [&renderer_ear, &audio_ear] {
        let heard = ear.heard();
        assert!(
            heard.lent_on.iter().all(|thread| *thread == here),
            "lent on the frames' thread: {:?}",
            heard.lent_on
        );
        assert!(
            !heard
                .asked
                .iter()
                .any(|request| (41..=43).contains(&request.epoch.0)),
            "a replay asks for no offer"
        );
    }
}

/// **Stopping a replay stops lending its epochs.** The DCC replays a trace of
/// thirty epochs while no frame runs (a long frame), so the epochs it has
/// published wait their turn; then the replay is stopped, returning to live
/// arbitration. The frames that follow do not go on lending the recorded
/// epochs still waiting, one per frame start: at most the one in force when
/// the replay stopped is lent.
#[test]
fn stopping_a_replay_stops_lending_its_epochs() {
    let mut host = host();
    let (renderer, ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    host.dcc.register_agent(renderer, 1.0);
    host.dcc.replay_decisions(DecisionTrace {
        epochs: (1_001..=1_030)
            .map(|epoch| {
                record(allocation(
                    epoch,
                    Trigger::Refresh,
                    vec![(AgentId::new("renderer"), cpu_envelope(3_000))],
                ))
            })
            .collect(),
    });
    host.start();

    // The long frame: the DCC publishes part of the trace meanwhile.
    thread::sleep(Duration::from_millis(150));
    let midway = host.dcc.is_replaying();
    host.dcc.stop_replay();
    host.frames_until(1, |_| false);
    host.dcc.stop();

    assert!(midway, "the replay was stopped before the trace ran out");
    let replayed: Vec<Epoch> = (ear.heard().lent.iter())
        .map(|(epoch, _, _)| *epoch)
        .filter(|epoch| (1_001..=1_030).contains(&epoch.0))
        .collect();
    assert!(
        replayed.len() <= 1,
        "recorded epochs lent after the replay stopped: {replayed:?}"
    );
}

/// **Replay lends every epoch even when a frame outlasts the DCC's ticks.**
/// The DCC replays one recorded epoch a tick; the scheduler lends at its
/// frame starts. A frame longer than the trace — a scene loading, a shader
/// compiling, here the first frame starting only once the DCC is done
/// replaying — still sees each recorded epoch lent to the agent, in order:
/// a replay that reproduces a session cannot drop the epochs a slow frame
/// stepped over.
#[test]
fn replay_lends_every_epoch_though_a_frame_outlasts_the_ticks() {
    let mut host = host();
    let (renderer, ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    host.dcc.register_agent(renderer, 1.0);
    host.dcc.replay_decisions(DecisionTrace {
        epochs: [(41, 3_000), (42, 4_000), (43, 5_000)]
            .iter()
            .map(|&(epoch, micros)| {
                record(allocation(
                    epoch,
                    Trigger::Refresh,
                    vec![(AgentId::new("renderer"), cpu_envelope(micros))],
                ))
            })
            .collect(),
    });
    host.start();

    // The long frame: no frame starts while the DCC replays.
    let replayed = wait_for(3, || !host.dcc.is_replaying());
    // Then frames run again, long enough for anything still owed to be lent.
    host.frames_until(1, |_| false);
    host.dcc.stop();

    assert!(replayed, "the DCC replayed the trace");
    let lent: Vec<(Epoch, Duration)> = ear
        .heard()
        .lent
        .iter()
        .filter(|(epoch, _, _)| (41..=43).contains(&epoch.0))
        .map(|(epoch, envelope, _)| (*epoch, envelope.time.cpu))
        .collect();
    let us = Duration::from_micros;
    assert_eq!(
        lent,
        vec![
            (Epoch(41), us(3_000)),
            (Epoch(42), us(4_000)),
            (Epoch(43), us(5_000))
        ],
        "every recorded epoch, once, in order: all lent {:?}",
        ear.heard().lent
    );
}

/// **A `Manual` mode set on the DCC pins the agent**: an agent that only
/// negotiates is applied its pinned `LowPower` (with ample budget it would
/// run `Balanced`), and a protocol agent is lent its pin.
#[test]
fn a_manual_mode_set_on_the_dcc_pins_the_agent() {
    let mut host = host();
    let legacy = Arc::new(Mutex::new(StubAgent { applied: None }));
    host.dcc.register_agent(legacy.clone(), 1.0);
    let (agent, ear) = Speaker::with_ear("physics", Duration::from_millis(2));
    host.dcc.register_agent(agent, 0.5);
    host.dcc.set_adaptation_mode(
        AgentId::new("renderer"),
        AdaptationMode::Manual(StrategyId::LowPower),
    );
    host.dcc.set_adaptation_mode(
        AgentId::new("physics"),
        AdaptationMode::Manual(StrategyId::LowPower),
    );
    host.start();

    let pinned = host.frames_until(3, |_| {
        ear.heard()
            .lent
            .iter()
            .any(|(_, _, pin)| *pin == Some(StrategyId::LowPower))
            && legacy.lock().expect("the agent lock").applied.is_some()
    });
    let applied = legacy.lock().expect("the agent lock").applied;
    host.dcc.stop();

    assert!(
        pinned,
        "the protocol agent is lent its pin: {:?}",
        ear.heard().lent
    );
    assert_eq!(
        applied,
        Some(StrategyId::LowPower),
        "the bridge applies the pin"
    );
}

/// **The governor's recovery widens the envelope again.** The agent offers a
/// 2 ms floor and an 8 ms upgrade: at the 16.66 ms target it is lent about
/// 14.9 ms. A sustained 40 ms frame pulls the budget multiplier down until
/// its envelope shrinks below 11 ms; once frames settle at 5 ms — a burst of
/// them each poll, so the frame time the governor remembers its cut for
/// passes quickly — only the governor can open an epoch, and the envelope
/// grows back above 12 ms. Without that trigger the agent would stay
/// squeezed forever.
#[test]
fn the_governors_recovery_widens_the_envelope_again() {
    let (mut dcc, rx) = recording(DccConfig::default());
    let (agent, _ear) = Speaker::with_upgrade(
        "renderer",
        Duration::from_millis(2),
        Duration::from_millis(8),
    );
    dcc.register_agent(agent, 1.0);
    let tx = dcc.event_sender();
    dcc.start(rx);
    let feed = |ms: f64, count: usize| {
        for _ in 0..count {
            tx.send(TelemetryEvent::FrameReport(frame_report(ms, None)))
                .expect("the DCC listens");
        }
    };
    let last_lent = || last_lent_to(&dcc, "renderer").map(|envelope| envelope.time.cpu);

    let squeezed = wait_for(5, || {
        feed(40.0, 1);
        last_lent().is_some_and(|cpu| cpu < Duration::from_millis(11))
    });
    assert!(
        squeezed,
        "a sustained overrun shrinks the envelope: {:?}",
        last_lent()
    );

    let widened = wait_for(10, || {
        feed(5.0, 10);
        last_lent().is_some_and(|cpu| cpu > Duration::from_millis(12))
    });
    let after = last_lent();
    dcc.stop();
    assert!(widened, "the recovery widens it again: {after:?}");
}

/// **The VRAM figure survives GPU timings.** A hardware report carrying GPU
/// timings says nothing of the VRAM: the figure the DCC knew stays, instead
/// of becoming `Some(0)` — which refused every upgrade that adds VRAM.
#[test]
fn vram_cap_is_not_zero_once_gpu_timings_arrive() {
    let (mut dcc, rx) = DccService::new(DccConfig {
        tick_rate: 100,
        ..Default::default()
    });
    let known = Some(256 * 1024 * 1024);
    dcc.context_handle()
        .write()
        .expect("the context lock")
        .hardware
        .available_vram = known;
    let tx = dcc.event_sender();
    dcc.start(rx);

    tx.send(TelemetryEvent::HardwareReport(
        khora_core::telemetry::monitoring::HardwareReport {
            thermal: khora_core::platform::ThermalStatus::Warm,
            battery: khora_core::platform::BatteryLevel::Mains,
            cpu_load: 0.1,
            gpu_load: Some(0.1),
            gpu_timings: Some(khora_core::telemetry::monitoring::GpuReport::default()),
        },
    ))
    .expect("the DCC listens");
    let ingested = wait_for(3, || {
        dcc.get_context().hardware.thermal == khora_core::platform::ThermalStatus::Warm
    });
    let vram = dcc.get_context().hardware.available_vram;
    dcc.stop();

    assert!(ingested, "the hardware report was ingested");
    assert_ne!(vram, Some(0));
    assert_eq!(
        vram, known,
        "a report without a VRAM figure keeps the known one"
    );
}

/// **Only the agents active in the mode are asked.** A script registered for
/// `Playing` alone is neither asked for an offer nor named by an epoch while
/// the editor edits; once the game plays, it is.
#[test]
fn only_agents_active_in_the_mode_are_asked() {
    let (mut dcc, rx) = recording(DccConfig {
        min_epoch_ticks: 1,
        ..Default::default()
    });
    let (always, _always_ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    let (script, script_ear) = Speaker::with_ear("script", Duration::from_millis(1));
    dcc.register_agent(always, 1.0);
    dcc.register_agent_for_mode(script, 0.5, vec![EngineMode::Playing]);
    let tx = dcc.event_sender();
    tx.send(TelemetryEvent::ModeChange(EngineMode::Custom(
        "editor".to_owned(),
    )))
    .expect("the DCC listens");
    dcc.start(rx);

    let editing = wait_for(3, || names(&dcc, "renderer"));
    let while_editing = epochs(&dcc);
    let script_asked_while_editing = script_ear.heard().asked.len();
    assert!(editing, "the agent active in every mode is named");
    assert_eq!(script_asked_while_editing, 0, "the script is not asked");
    assert!(
        while_editing.iter().all(|epoch| epoch
            .envelopes
            .iter()
            .all(|(id, _)| *id != AgentId::new("script"))),
        "no envelope for the script: {while_editing:?}"
    );

    tx.send(TelemetryEvent::ModeChange(EngineMode::Playing))
        .expect("the DCC listens");
    let playing = wait_for(3, || names(&dcc, "script"));
    dcc.stop();
    assert!(playing, "once the game plays, the script is named");
    assert!(!script_ear.heard().asked.is_empty());
}

/// **The journal records each epoch's inputs.** Before the first epoch the
/// developer set the renderer `Stable` and capped it at 12 ms, the scheduler
/// published its waves, and the renderer reported a run. The first epoch's
/// record holds them all — the target the renderer was asked for, its offer
/// for that epoch, its report, the waves, the policy and the hint — beside
/// the allocation.
#[test]
fn the_journal_records_each_epochs_inputs() {
    let (mut dcc, rx) = recording(DccConfig::default());
    let renderer = AgentId::new("renderer");
    let (agent, ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    dcc.register_agent(agent, 1.0);
    dcc.set_adaptation_mode(renderer, AdaptationMode::Stable);
    dcc.set_hint(khora_core::agent::gorna::EngineHint::Cap {
        agent: renderer,
        max_ms: 12.0,
    });
    let tx = dcc.event_sender();
    let report = Report {
        agent: renderer,
        epoch: Epoch(0),
        row: StrategyId::LowPower,
        measured: PerDevice {
            cpu: Duration::from_millis(2),
            ..PerDevice::default()
        },
        done: 1.0,
        health: 1.0,
        over_budget: false,
        stalled: false,
        workload: Some(10.0),
        switch: None,
    };
    tx.send(TelemetryEvent::AgentReport(report.clone()))
        .expect("the DCC listens");
    tx.send(TelemetryEvent::WavePlan {
        waves: vec![vec![renderer]],
    })
    .expect("the DCC listens");
    dcc.start(rx);

    let recorded = wait_for(3, || !records(&dcc).is_empty());
    let first = records(&dcc).first().cloned();
    dcc.stop();

    assert!(recorded, "the first epoch was journaled");
    let first = first.expect("the first record");
    let asked = ear
        .heard()
        .asked
        .first()
        .cloned()
        .expect("the renderer was asked");
    assert_eq!(first.allocation.trigger, Trigger::Initial);
    assert_eq!(first.target, asked.target, "the target the round shared");
    assert!(
        first.offers.iter().any(|offer| offer.agent == renderer
            && offer.epoch == first.allocation.epoch
            && !offer.rows.is_empty()),
        "the renderer's offer for the epoch: {:?}",
        first.offers
    );
    assert_eq!(first.reports, vec![report], "the renderer's latest report");
    assert_eq!(first.waves, vec![vec![renderer]]);
    assert!(
        first.policies.contains(&(renderer, AdaptationMode::Stable)),
        "{:?}",
        first.policies
    );
    assert!(
        first
            .hints
            .iter()
            .any(|(id, hints)| *id == renderer && hints.cap_ms == Some(12.0)),
        "{:?}",
        first.hints
    );
}

/// **A channel connected after the DCC started still carries its
/// allocations.** The engine starts the decision thread, then builds its
/// scheduler and connects its channel: the agent is lent its envelope at a
/// frame start all the same.
#[test]
fn a_channel_connected_after_the_dcc_started_carries_its_allocations() {
    let (mut dcc, rx) = DccService::new(DccConfig {
        tick_rate: 100,
        ..Default::default()
    });
    let (agent, ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    dcc.register_agent(agent, 1.0);
    dcc.start(rx);
    let mut scheduler = crate::scheduler::ExecutionScheduler::from_registry(
        Arc::clone(dcc.agent_registry()),
        dcc.context_handle(),
    );
    dcc.connect_budget_channel(scheduler.budget_channel().clone());
    let mut world = khora_data::ecs::World::new();
    let runtime = Arc::new(khora_core::Runtime::new());

    let lent = wait_for(3, || {
        scheduler.run_frame(&mut world, Arc::clone(&runtime));
        !ear.heard().lent.is_empty()
    });
    dcc.stop();

    assert!(lent, "the agent was lent an envelope");
}

/// **An allocation decided before the scheduler connected is not lost.** The
/// first epoch opens while no channel is connected yet; once the scheduler
/// connects, the allocation in force is handed to it, and the agent is lent
/// its envelope without waiting for another epoch to open.
#[test]
fn an_allocation_decided_before_the_scheduler_connected_is_lent() {
    let (mut dcc, rx) = recording(DccConfig {
        tick_rate: 100,
        ..Default::default()
    });
    let (agent, ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    dcc.register_agent(agent, 1.0);
    dcc.start(rx);
    assert!(
        wait_for(3, || !epochs(&dcc).is_empty()),
        "the first epoch opened"
    );
    let mut scheduler = crate::scheduler::ExecutionScheduler::from_registry(
        Arc::clone(dcc.agent_registry()),
        dcc.context_handle(),
    );
    dcc.connect_budget_channel(scheduler.budget_channel().clone());
    let mut world = khora_data::ecs::World::new();
    let runtime = Arc::new(khora_core::Runtime::new());

    let lent = wait_for(3, || {
        scheduler.run_frame(&mut world, Arc::clone(&runtime));
        !ear.heard().lent.is_empty()
    });
    let opened = epochs(&dcc).len();
    dcc.stop();

    assert!(lent, "the allocation in force was lent");
    assert_eq!(opened, 1, "no other epoch was needed");
}
