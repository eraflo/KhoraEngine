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

//! When the running DCC opens an epoch: a mode change, a new agent, agents
//! over budget, a stalled agent — never sooner than the epoch's minimum,
//! except for `Safety`.

use super::*;

/// A DCC whose epochs may follow one another every tick, holding one
/// protocol agent with a 2 ms floor.
fn one_speaker(config: DccConfig) -> (DccService, Receiver<TelemetryEvent>, Ear) {
    let (dcc, rx) = recording(config);
    let (agent, ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    dcc.register_agent(agent, 1.0);
    (dcc, rx, ear)
}

fn quick() -> DccConfig {
    DccConfig {
        min_epoch_ticks: 1,
        ..Default::default()
    }
}

/// **A mode change opens an epoch**, with the `ModeChanged` trigger.
#[test]
fn a_mode_change_opens_an_epoch() {
    let (mut dcc, rx, _ear) = one_speaker(quick());
    let tx = dcc.event_sender();
    dcc.start(rx);
    assert!(
        wait_for(3, || opened_by(&dcc, Trigger::Initial)),
        "the first epoch"
    );

    tx.send(TelemetryEvent::ModeChange(EngineMode::Custom(
        "editor".to_owned(),
    )))
    .expect("the DCC listens");
    let opened = wait_for(3, || opened_by(&dcc, Trigger::ModeChanged));
    let recorded = epochs(&dcc);
    dcc.stop();

    assert!(opened, "{recorded:?}");
}

/// **Registering an agent opens an epoch**, `AgentsChanged`, which lends
/// the newcomer an envelope.
#[test]
fn registering_an_agent_opens_an_epoch() {
    let (mut dcc, rx, _ear) = one_speaker(quick());
    dcc.start(rx);
    assert!(wait_for(3, || opened_by(&dcc, Trigger::Initial)));

    let (late_comer, ear) = Speaker::with_ear("audio", Duration::from_millis(1));
    dcc.register_agent(late_comer, 0.5);
    let opened = wait_for(3, || opened_by(&dcc, Trigger::AgentsChanged));
    let lent = wait_for(1, || !ear.heard().lent.is_empty());
    let recorded = epochs(&dcc);
    dcc.stop();

    assert!(opened, "{recorded:?}");
    assert!(lent, "the newcomer is lent an envelope");
}

/// **Over-budget reports in a row open an epoch**, `Overruns`, once there
/// are as many as the configuration says.
#[test]
fn over_budget_reports_in_a_row_open_an_epoch() {
    let (mut dcc, rx, ear) = one_speaker(DccConfig {
        overruns_in_a_row: 3,
        ..quick()
    });
    ear.health().over_budget_reports = u32::MAX;
    dcc.start(rx);

    let opened = wait_for(3, || opened_by(&dcc, Trigger::Overruns));
    let recorded = epochs(&dcc);
    dcc.stop();

    assert!(opened, "{recorded:?}");
}

/// **Fewer over-budget reports than the limit open no epoch.** Four reports
/// over budget, then none, against a limit of five.
#[test]
fn fewer_over_budget_reports_than_the_limit_open_no_epoch() {
    let (mut dcc, rx, ear) = one_speaker(DccConfig {
        overruns_in_a_row: 5,
        ..quick()
    });
    ear.health().over_budget_reports = 4;
    dcc.start(rx);

    let first = wait_for(3, || opened_by(&dcc, Trigger::Initial));
    thread::sleep(Duration::from_millis(300));
    let recorded = epochs(&dcc);
    dcc.stop();

    assert!(first, "the first epoch was recorded");
    assert!(
        recorded
            .iter()
            .all(|epoch| epoch.trigger != Trigger::Overruns),
        "{recorded:?}"
    );
}

/// **An epoch lasts at least its minimum.** With a minimum of 30 ticks
/// (300 ms at 100 Hz), a mode change right after the first epoch opens no
/// epoch at once; it is not lost either: its epoch opens once the minimum
/// has passed.
#[test]
fn an_epoch_lasts_at_least_its_minimum_ticks() {
    let (mut dcc, rx, _ear) = one_speaker(DccConfig {
        min_epoch_ticks: 30,
        ..Default::default()
    });
    let tx = dcc.event_sender();
    dcc.start(rx);
    assert!(wait_for(3, || opened_by(&dcc, Trigger::Initial)));

    tx.send(TelemetryEvent::ModeChange(EngineMode::Custom(
        "editor".to_owned(),
    )))
    .expect("the DCC listens");
    thread::sleep(Duration::from_millis(120));
    let too_soon = epochs(&dcc);
    let later = wait_for(5, || opened_by(&dcc, Trigger::ModeChanged));
    dcc.stop();

    assert_eq!(
        too_soon.len(),
        1,
        "no second epoch within 12 ticks: {too_soon:?}"
    );
    assert!(later, "the mode change opens its epoch after the minimum");
}

/// **`Safety` does not wait for the epoch to end.** Under a minimum of
/// 100 000 ticks, a death spiral (critical heat, critical battery, a
/// saturated processor) opens a `Safety` epoch at once, lending the agent
/// its floor.
#[test]
fn safety_does_not_wait_for_the_epoch_to_end() {
    let (mut dcc, rx, _ear) = one_speaker(DccConfig {
        min_epoch_ticks: 100_000,
        ..Default::default()
    });
    let tx = dcc.event_sender();
    dcc.start(rx);
    assert!(wait_for(3, || opened_by(&dcc, Trigger::Initial)));

    tx.send(TelemetryEvent::HardwareReport(
        khora_core::telemetry::monitoring::HardwareReport {
            thermal: khora_core::platform::ThermalStatus::Critical,
            battery: khora_core::platform::BatteryLevel::Critical,
            cpu_load: 0.99,
            gpu_load: Some(0.2),
            gpu_timings: None,
        },
    ))
    .expect("the DCC listens");
    let opened = wait_for(3, || opened_by(&dcc, Trigger::Safety));
    let safety = epochs(&dcc)
        .into_iter()
        .find(|epoch| epoch.trigger == Trigger::Safety);
    dcc.stop();

    assert!(opened, "the emergency opened its epoch");
    let safety = safety.expect("the Safety epoch");
    let lent = safety
        .envelopes
        .iter()
        .find(|(id, _)| *id == AgentId::new("renderer"))
        .map(|(_, envelope)| *envelope)
        .expect("the agent is lent its floor");
    assert_eq!(
        lent.time.cpu,
        Duration::from_millis(2),
        "the floor: {lent:?}"
    );
}

/// **A stalled, unhealthy agent opens an epoch**, `AgentStalled`, where a
/// health below 0.5 was once only logged.
#[test]
fn a_stalled_unhealthy_agent_opens_an_epoch() {
    let (mut dcc, rx, ear) = one_speaker(quick());
    dcc.start(rx);
    assert!(wait_for(3, || opened_by(&dcc, Trigger::Initial)));

    {
        let mut health = ear.health();
        health.stalled = true;
        health.health = 0.2;
    }
    let opened = wait_for(3, || opened_by(&dcc, Trigger::AgentStalled));
    let recorded = epochs(&dcc);
    dcc.stop();

    assert!(opened, "{recorded:?}");
}

/// An agent written before the protocol whose health the test sets: it
/// reports through the bridge, so its reports carry no epoch.
struct Fading {
    health: Arc<Mutex<Health>>,
}

impl Agent for Fading {
    fn id(&self) -> AgentId {
        AgentId::new("audio")
    }
    fn negotiate(&mut self, _: NegotiationRequest) -> NegotiationResponse {
        NegotiationResponse {
            strategies: vec![StrategyOption {
                id: StrategyId::Balanced,
                estimated_time: Duration::from_millis(1),
                estimated_vram: 0,
            }],
            timing_adjustment: None,
        }
    }
    fn apply_budget(&mut self, _: ResourceBudget) {}
    fn report_status(&self) -> AgentStatus {
        let health = self.health.lock().expect("the health lock");
        AgentStatus {
            agent_id: AgentId::new("audio"),
            current_strategy: StrategyId::Balanced,
            health_score: health.health,
            is_stalled: health.stalled,
            message: String::new(),
        }
    }
    fn execute(&mut self, _: &mut khora_core::EngineContext<'_>) {}
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// **A bridged agent's report is not stale.** Its reports say `Epoch(0)`
/// whatever epoch is in force; the DCC still reads them, so when it stalls
/// and falls below 0.5 it opens an `AgentStalled` epoch like any other.
#[test]
fn a_bridged_agents_report_is_not_stale() {
    let (mut dcc, rx) = recording(quick());
    let health = Arc::new(Mutex::new(Health::default()));
    dcc.register_agent(
        Arc::new(Mutex::new(Fading {
            health: Arc::clone(&health),
        })),
        1.0,
    );
    dcc.start(rx);
    assert!(wait_for(3, || opened_by(&dcc, Trigger::Initial)));

    {
        let mut health = health.lock().expect("the health lock");
        health.stalled = true;
        health.health = 0.2;
    }
    let opened = wait_for(3, || opened_by(&dcc, Trigger::AgentStalled));
    let recorded = epochs(&dcc);
    dcc.stop();

    assert!(opened, "{recorded:?}");
}

/// **An agent busy when an epoch opens does not reopen it.** The set of
/// agents does not change; one of them is running (its lock held) at the
/// moment the epoch opens. The next tick reads every agent and finds the
/// same set: no `AgentsChanged` is raised.
#[test]
fn an_agent_busy_when_an_epoch_opens_does_not_reopen_it() {
    use super::super::epochs::Epochs;

    let (renderer, _renderer_ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    let (audio, _audio_ear) = Speaker::with_ear("audio", Duration::from_millis(1));
    let agents = vec![renderer, Arc::clone(&audio)];
    let mut tracker = Epochs::default();

    tracker.observe(&agents, 3);
    {
        let _running = audio.lock().expect("the agent lock");
        tracker.opened(&agents);
    }
    tracker.observe(&agents, 3);

    assert_eq!(tracker.due(0), None, "the same agents, no new epoch");
}

/// **A mode change during a replay opens an epoch once the replay ends.**
/// The replay lends recorded allocations for 20 ticks; the engine changes
/// mode meanwhile. The trigger is not dropped: when live arbitration
/// resumes, a `ModeChanged` epoch opens for the agents of the new mode.
#[test]
fn a_mode_change_during_a_replay_opens_an_epoch_after_it() {
    let (mut dcc, rx, _ear) = one_speaker(quick());
    let recorded = |epoch| Allocation {
        epoch: Epoch(epoch),
        trigger: Trigger::Refresh,
        envelopes: Vec::new(),
        pins: Vec::new(),
        allowed: Vec::new(),
        price: None,
        late: Vec::new(),
    };
    dcc.replay_decisions(DecisionTrace {
        epochs: (1..=20).map(recorded).collect(),
    });
    let tx = dcc.event_sender();
    dcc.start(rx);
    tx.send(TelemetryEvent::ModeChange(EngineMode::Custom(
        "editor".to_owned(),
    )))
    .expect("the DCC listens");

    let replayed = wait_for(3, || !dcc.is_replaying());
    let opened = wait_for(2, || opened_by(&dcc, Trigger::ModeChanged));
    let live = epochs(&dcc);
    dcc.stop();

    assert!(replayed, "the replay ran out");
    assert!(opened, "the mode change opened its epoch: {live:?}");
}

/// **An agent replaced by another opens an epoch**, even when the newcomer's
/// lock sits at the address the removed agent's had.
#[test]
fn an_agent_replaced_by_another_opens_an_epoch() {
    use super::super::epochs::Epochs;

    let address = |agent: &Arc<Mutex<dyn Agent>>| Arc::as_ptr(agent) as *const () as usize;
    let mut tracker = Epochs::default();
    let (renderer, renderer_ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    let freed = address(&renderer);
    tracker.observe(std::slice::from_ref(&renderer), 3);
    tracker.opened(std::slice::from_ref(&renderer));
    drop(renderer);
    drop(renderer_ear);

    // Allocate until a new agent's lock lands where the removed one's was.
    let mut elsewhere = Vec::new();
    let audio = loop {
        let (audio, ear) = Speaker::with_ear("audio", Duration::from_millis(1));
        if address(&audio) == freed || elsewhere.len() >= 4096 {
            break audio;
        }
        elsewhere.push((audio, ear));
    };
    let reused = address(&audio) == freed;
    tracker.observe(std::slice::from_ref(&audio), 3);

    assert_eq!(
        tracker.due(0),
        Some(Trigger::AgentsChanged),
        "address reused: {reused}"
    );
}

/// **An agent a replay did not name is lent an envelope after it.** The
/// recorded allocations name no agent; once the replay is over, live
/// arbitration lends the registered agent an envelope.
#[test]
fn an_agent_the_replay_did_not_name_is_lent_an_envelope_after_it() {
    let (mut dcc, rx, ear) = one_speaker(quick());
    let recorded = |epoch| Allocation {
        epoch: Epoch(epoch),
        trigger: Trigger::Refresh,
        envelopes: Vec::new(),
        pins: Vec::new(),
        allowed: Vec::new(),
        price: None,
        late: Vec::new(),
    };
    dcc.replay_decisions(DecisionTrace {
        epochs: (1..=5).map(recorded).collect(),
    });
    dcc.start(rx);

    let replayed = wait_for(3, || !dcc.is_replaying());
    let lent = wait_for(2, || !ear.heard().lent.is_empty());
    let live = epochs(&dcc);
    dcc.stop();

    assert!(replayed, "the replay ran out");
    assert!(lent, "the agent is lent an envelope: {live:?}");
}
