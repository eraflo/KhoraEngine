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

//! What the running DCC says to its agents: offers asked, envelopes lent,
//! policies, the PID's effect on the envelopes, replay — and only to the
//! agents active in the engine's mode.

use super::*;

/// **The first epoch lends every agent an envelope**, through the protocol:
/// the agent is asked for an offer and applied an envelope, never negotiated
/// with nor applied a budget. The first allocation is `Initial`.
#[test]
fn the_first_epoch_lends_every_agent_an_envelope() {
    let (mut dcc, rx) = recording(DccConfig::default());
    let (agent, ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    dcc.register_agent(agent, 1.0);
    dcc.start(rx);

    let lent = wait_for(3, || !ear.heard().lent.is_empty());
    let first = epochs(&dcc).first().cloned();
    dcc.stop();

    assert!(lent, "the agent was lent an envelope");
    let heard = ear.heard();
    assert!(!heard.asked.is_empty(), "the agent was asked for an offer");
    assert_eq!(heard.old_calls, 0, "never negotiated with nor budgeted");
    let first = first.expect("the epoch was recorded");
    assert_eq!(first.trigger, Trigger::Initial);
    assert_eq!(heard.lent[0].0, first.epoch, "lent for the epoch recorded");
}

/// **The DCC records each epoch's allocation**: every recorded epoch lends
/// the registered agent — one that only negotiates — an envelope.
#[test]
fn the_dcc_records_each_epochs_allocation() {
    let (mut dcc, rx) = recording(DccConfig::default());
    let agent = Arc::new(Mutex::new(StubAgent { applied: None }));
    dcc.register_agent(agent.clone(), 1.0);
    dcc.start(rx);

    let recorded = wait_for(3, || !epochs(&dcc).is_empty());
    let trace = dcc.stop_decision_recording();
    dcc.stop();

    assert!(recorded, "recording captured at least one epoch");
    assert!(
        trace.epochs.iter().all(|epoch| epoch
            .envelopes
            .iter()
            .any(|(id, _)| *id == AgentId::new("renderer"))),
        "every recorded epoch lends the registered agent: {:?}",
        trace.epochs
    );
    assert!(
        agent.lock().expect("the agent lock").applied.is_some(),
        "the bridge applied a budget"
    );
}

#[test]
fn replay_runs_while_the_trace_has_epochs_left() {
    let (dcc, _rx) = DccService::new(DccConfig::default());
    assert!(!dcc.is_replaying());

    let trace = DecisionTrace {
        epochs: vec![Allocation {
            epoch: Epoch(1),
            trigger: Trigger::Initial,
            envelopes: Vec::new(),
            pins: Vec::new(),
            allowed: Vec::new(),
            price: None,
            late: Vec::new(),
        }],
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
    let (mut dcc, rx) = recording(DccConfig::default());
    let (agent, ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    dcc.register_agent(agent, 1.0);
    let recorded = Envelope {
        time: PerDevice {
            cpu: Duration::from_micros(3_333),
            ..PerDevice::default()
        },
        ..Envelope::default()
    };
    dcc.replay_decisions(DecisionTrace {
        epochs: vec![Allocation {
            epoch: Epoch(42),
            trigger: Trigger::Refresh,
            envelopes: vec![(AgentId::new("renderer"), recorded)],
            pins: Vec::new(),
            allowed: Vec::new(),
            price: None,
            late: Vec::new(),
        }],
    });
    dcc.start(rx);

    let replayed = wait_for(3, || {
        ear.heard()
            .lent
            .iter()
            .any(|(_, envelope, _)| *envelope == recorded)
    });
    dcc.stop();

    assert!(replayed, "lent: {:?}", ear.heard().lent);
}

/// **A `Manual` mode set on the DCC pins the agent**: an agent that only
/// negotiates is applied its pinned `LowPower` (with ample budget it would
/// run `Balanced`), and a protocol agent is lent its pin.
#[test]
fn a_manual_mode_set_on_the_dcc_pins_the_agent() {
    let (mut dcc, rx) = recording(DccConfig::default());
    let legacy = Arc::new(Mutex::new(StubAgent { applied: None }));
    dcc.register_agent(legacy.clone(), 1.0);
    let (agent, ear) = Speaker::with_ear("physics", Duration::from_millis(2));
    dcc.register_agent(agent, 0.5);
    dcc.set_adaptation_mode(
        AgentId::new("renderer"),
        AdaptationMode::Manual(StrategyId::LowPower),
    );
    dcc.set_adaptation_mode(
        AgentId::new("physics"),
        AdaptationMode::Manual(StrategyId::LowPower),
    );
    dcc.start(rx);

    let pinned = wait_for(3, || {
        ear.heard()
            .lent
            .iter()
            .any(|(_, _, pin)| *pin == Some(StrategyId::LowPower))
    });
    let applied = legacy.lock().expect("the agent lock").applied;
    dcc.stop();

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

/// **The PID's recovery widens the envelope again.** A sustained 40 ms frame
/// pulls the budget multiplier down until the agent's envelope shrinks below
/// 11 ms (a 2 ms floor in 16.66 ms is lent about 13.7 ms); once frames
/// settle at 5 ms, only the PID's drift can open an epoch, and the envelope
/// grows back above 12 ms. Without that trigger the agent would stay
/// squeezed forever.
#[test]
fn the_pid_recovery_widens_the_envelope_again() {
    let (mut dcc, rx) = recording(DccConfig::default());
    let (agent, ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    dcc.register_agent(agent, 1.0);
    let tx = dcc.event_sender();
    dcc.start(rx);
    let frame_time_id = MetricId::new("renderer", "frame_time");
    let feed = |ms: f64| {
        tx.send(TelemetryEvent::MetricUpdate {
            id: frame_time_id.clone(),
            value: MetricValue::Gauge(ms),
        })
        .expect("the DCC listens");
    };
    let last_lent = || {
        ear.heard()
            .lent
            .last()
            .map(|(_, envelope, _)| envelope.time.cpu)
    };

    let squeezed = wait_for(5, || {
        feed(40.0);
        last_lent().is_some_and(|cpu| cpu < Duration::from_millis(11))
    });
    assert!(
        squeezed,
        "a sustained overrun shrinks the envelope: {:?}",
        last_lent()
    );

    let widened = wait_for(10, || {
        feed(5.0);
        last_lent().is_some_and(|cpu| cpu > Duration::from_millis(12))
    });
    dcc.stop();
    assert!(widened, "the recovery widens it again: {:?}", last_lent());
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
/// `Playing` alone is neither asked for an offer nor lent an envelope while
/// the editor edits; once the game plays, it is.
#[test]
fn only_agents_active_in_the_mode_are_asked() {
    let (mut dcc, rx) = recording(DccConfig {
        min_epoch_ticks: 1,
        ..Default::default()
    });
    let (always, always_ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    let (script, script_ear) = Speaker::with_ear("script", Duration::from_millis(1));
    dcc.register_agent(always, 1.0);
    dcc.register_agent_for_mode(script, 0.5, vec![EngineMode::Playing]);
    let tx = dcc.event_sender();
    tx.send(TelemetryEvent::ModeChange(EngineMode::Custom(
        "editor".to_owned(),
    )))
    .expect("the DCC listens");
    dcc.start(rx);

    let editing = wait_for(3, || !always_ear.heard().lent.is_empty());
    let while_editing = epochs(&dcc);
    let script_asked_while_editing = script_ear.heard().asked.len();
    assert!(
        editing,
        "the agent active in every mode is lent an envelope"
    );
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
    let playing = wait_for(3, || !script_ear.heard().lent.is_empty());
    dcc.stop();
    assert!(
        playing,
        "once the game plays, the script is lent an envelope"
    );
    assert!(!script_ear.heard().asked.is_empty());
}
