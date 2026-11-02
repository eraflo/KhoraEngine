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

/// **Registering an agent opens an epoch**, `AgentsChanged`, which names the
/// newcomer.
#[test]
fn registering_an_agent_opens_an_epoch() {
    let (mut dcc, rx, _ear) = one_speaker(quick());
    dcc.start(rx);
    assert!(wait_for(3, || opened_by(&dcc, Trigger::Initial)));

    let (late_comer, _late_ear) = Speaker::with_ear("audio", Duration::from_millis(1));
    dcc.register_agent(late_comer, 0.5);
    let opened = wait_for(3, || opened_by(&dcc, Trigger::AgentsChanged));
    let recorded = epochs(&dcc);
    dcc.stop();

    assert!(opened, "{recorded:?}");
    assert!(
        recorded
            .iter()
            .filter(|epoch| epoch.trigger == Trigger::AgentsChanged)
            .all(|epoch| epoch
                .envelopes
                .iter()
                .any(|(id, _)| *id == AgentId::new("audio"))),
        "the newcomer is named: {recorded:?}"
    );
}

/// `report` on `epoch`, over budget or not.
fn over(report: Report, epoch: Epoch, over_budget: bool) -> TelemetryEvent {
    TelemetryEvent::AgentReport(Report {
        epoch,
        over_budget,
        ..report
    })
}

/// The renderer's report of a 2 ms run on its floor, healthy, within budget.
fn renderer_report() -> Report {
    Report {
        agent: AgentId::new("renderer"),
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
        workload: None,
        switch: None,
    }
}

/// The epoch the first allocation opened, once it has.
fn first_epoch(dcc: &DccService) -> Epoch {
    assert!(wait_for(3, || opened_by(dcc, Trigger::Initial)));
    epochs(dcc)[0].epoch
}

/// **Over-budget reports in a row open an epoch**, `Overruns`, once as many
/// as the configuration says are published — the reports the scheduler
/// publishes, not ones the DCC asks the agent for (asked, this agent says it
/// is within budget).
#[test]
fn over_budget_reports_in_a_row_open_an_epoch() {
    let (mut dcc, rx, _ear) = one_speaker(DccConfig {
        overruns_in_a_row: 3,
        ..quick()
    });
    let tx = dcc.event_sender();
    dcc.start(rx);
    let epoch = first_epoch(&dcc);

    for _ in 0..3 {
        tx.send(over(renderer_report(), epoch, true))
            .expect("the DCC listens");
    }
    let opened = wait_for(3, || opened_by(&dcc, Trigger::Overruns));
    let recorded = epochs(&dcc);
    dcc.stop();

    assert!(opened, "{recorded:?}");
}

/// **Fewer over-budget reports in a row than the limit open no epoch.**
/// Against a limit of five: four reports over budget, one within, four over
/// — eight over, never five in a row.
#[test]
fn fewer_over_budget_reports_than_the_limit_open_no_epoch() {
    let (mut dcc, rx, _ear) = one_speaker(DccConfig {
        overruns_in_a_row: 5,
        ..quick()
    });
    let tx = dcc.event_sender();
    dcc.start(rx);
    let epoch = first_epoch(&dcc);

    for over_budget in [true, true, true, true, false, true, true, true, true] {
        tx.send(over(renderer_report(), epoch, over_budget))
            .expect("the DCC listens");
    }
    thread::sleep(Duration::from_millis(300));
    let recorded = epochs(&dcc);
    dcc.stop();

    assert!(
        recorded
            .iter()
            .all(|epoch| epoch.trigger != Trigger::Overruns),
        "{recorded:?}"
    );
}

/// **The DCC counts overruns from published reports, not by asking agents.**
/// Asked, this agent would say it is over budget every time; no report is
/// published, so no `Overruns` epoch opens.
#[test]
fn an_agent_over_budget_only_when_asked_opens_no_epoch() {
    let (mut dcc, rx, ear) = one_speaker(DccConfig {
        overruns_in_a_row: 3,
        ..quick()
    });
    ear.health().over_budget_reports = u32::MAX;
    dcc.start(rx);
    first_epoch(&dcc);

    thread::sleep(Duration::from_millis(300));
    let recorded = epochs(&dcc);
    dcc.stop();

    assert!(
        recorded
            .iter()
            .all(|epoch| epoch.trigger != Trigger::Overruns),
        "{recorded:?}"
    );
}

/// **A bridged agent's reports count toward overruns.** An agent bridged from
/// the old protocol reports `Epoch(0)` whatever epoch is in force; its
/// over-budget reports in a row are not dropped as stale, and open an
/// `Overruns` epoch like any other agent's.
#[test]
fn reports_of_epoch_zero_count_toward_overruns() {
    let (mut dcc, rx, _ear) = one_speaker(DccConfig {
        overruns_in_a_row: 3,
        ..quick()
    });
    let tx = dcc.event_sender();
    dcc.start(rx);
    assert!(first_epoch(&dcc) > Epoch(0), "an epoch is in force");

    for _ in 0..3 {
        tx.send(over(renderer_report(), Epoch(0), true))
            .expect("the DCC listens");
    }
    let opened = wait_for(3, || opened_by(&dcc, Trigger::Overruns));
    let recorded = epochs(&dcc);
    dcc.stop();

    assert!(opened, "{recorded:?}");
}

/// **The overruns an epoch answered open no other.** Three over-budget
/// reports on the first epoch open an `Overruns` epoch. The scheduler lends
/// it only at its next frame start, so the runs of the frames still going
/// while the round decided — over budget, on the first epoch — reach the DCC
/// after the epoch opened (a physics agent sub-stepping three times a frame
/// sends three in one frame). Then the agent, lent the new epoch, runs within
/// it. The overruns were already answered: no second `Overruns` epoch opens.
#[test]
fn the_overruns_an_epoch_answered_open_no_other() {
    let (mut dcc, rx, _ear) = one_speaker(DccConfig {
        overruns_in_a_row: 3,
        ..quick()
    });
    let tx = dcc.event_sender();
    dcc.start(rx);
    let first = first_epoch(&dcc);

    for _ in 0..3 {
        tx.send(over(renderer_report(), first, true))
            .expect("the DCC listens");
    }
    assert!(wait_for(3, || opened_by(&dcc, Trigger::Overruns)));
    let answered = (epochs(&dcc).iter())
        .find(|epoch| epoch.trigger == Trigger::Overruns)
        .map(|epoch| epoch.epoch)
        .expect("the Overruns epoch");

    // The frames that ran on the first epoch while the round decided.
    for _ in 0..3 {
        tx.send(over(renderer_report(), first, true))
            .expect("the DCC listens");
    }
    // Lent the new epoch, the agent runs within it.
    for _ in 0..3 {
        tx.send(over(renderer_report(), answered, false))
            .expect("the DCC listens");
    }
    thread::sleep(Duration::from_millis(300));
    let recorded = epochs(&dcc);
    dcc.stop();

    let overruns = (recorded.iter())
        .filter(|epoch| epoch.trigger == Trigger::Overruns)
        .count();
    assert_eq!(overruns, 1, "{recorded:?}");
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
/// health below 0.5 was once only logged — learnt from its published report
/// (asked, this agent says it is well).
#[test]
fn a_stalled_unhealthy_agent_opens_an_epoch() {
    let (mut dcc, rx, _ear) = one_speaker(quick());
    let tx = dcc.event_sender();
    dcc.start(rx);
    let epoch = first_epoch(&dcc);

    tx.send(TelemetryEvent::AgentReport(Report {
        epoch,
        health: 0.2,
        stalled: true,
        ..renderer_report()
    }))
    .expect("the DCC listens");
    let opened = wait_for(3, || opened_by(&dcc, Trigger::AgentStalled));
    let recorded = epochs(&dcc);
    dcc.stop();

    assert!(opened, "{recorded:?}");
}

/// **A stalled agent in good health opens no epoch**: a stall alone, at a
/// health of 0.9, is not a reason to share the frame again.
#[test]
fn a_stalled_agent_in_good_health_opens_no_epoch() {
    let (mut dcc, rx, _ear) = one_speaker(quick());
    let tx = dcc.event_sender();
    dcc.start(rx);
    let epoch = first_epoch(&dcc);

    tx.send(TelemetryEvent::AgentReport(Report {
        epoch,
        health: 0.9,
        stalled: true,
        ..renderer_report()
    }))
    .expect("the DCC listens");
    thread::sleep(Duration::from_millis(300));
    let recorded = epochs(&dcc);
    dcc.stop();

    assert!(
        recorded
            .iter()
            .all(|epoch| epoch.trigger != Trigger::AgentStalled),
        "{recorded:?}"
    );
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
/// whatever epoch is in force; the scheduler publishes them after each run
/// and the DCC reads them, so when it stalls and falls below 0.5 it opens an
/// `AgentStalled` epoch like any other.
#[test]
fn a_bridged_agents_report_is_not_stale() {
    let mut host = Host::new(DccConfig {
        tick_rate: 100,
        ..quick()
    });
    let health = Arc::new(Mutex::new(Health::default()));
    host.dcc.register_agent(
        Arc::new(Mutex::new(Fading {
            health: Arc::clone(&health),
        })),
        1.0,
    );
    host.start();
    assert!(host.frames_until(3, |dcc| opened_by(dcc, Trigger::Initial)));

    {
        let mut health = health.lock().expect("the health lock");
        health.stalled = true;
        health.health = 0.2;
    }
    let opened = host.frames_until(3, |dcc| opened_by(dcc, Trigger::AgentStalled));
    let recorded = epochs(&host.dcc);
    host.dcc.stop();

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

    tracker.observe(&agents);
    {
        let _running = audio.lock().expect("the agent lock");
        tracker.opened(&agents);
    }
    tracker.observe(&agents);

    assert_eq!(tracker.due(0), None, "the same agents, no new epoch");
}

/// **A mode change during a replay opens an epoch once the replay ends.**
/// The replay lends recorded allocations for 20 ticks; the engine changes
/// mode meanwhile. The trigger is not dropped: when live arbitration
/// resumes, a `ModeChanged` epoch opens for the agents of the new mode.
#[test]
fn a_mode_change_during_a_replay_opens_an_epoch_after_it() {
    let (mut dcc, rx, _ear) = one_speaker(quick());
    dcc.replay_decisions(DecisionTrace {
        epochs: (1..=20)
            .map(|epoch| record(allocation(epoch, Trigger::Refresh, Vec::new())))
            .collect(),
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
    tracker.observe(std::slice::from_ref(&renderer));
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
    tracker.observe(std::slice::from_ref(&audio));

    assert_eq!(
        tracker.due(0),
        Some(Trigger::AgentsChanged),
        "address reused: {reused}"
    );
}

/// **An agent a replay did not name is lent an envelope after it.** The
/// recorded allocations name no agent; once the replay is over, live
/// arbitration names the registered agent, and the scheduler lends it.
#[test]
fn an_agent_the_replay_did_not_name_is_lent_an_envelope_after_it() {
    let mut host = Host::new(DccConfig {
        tick_rate: 100,
        ..quick()
    });
    let (agent, ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    host.dcc.register_agent(agent, 1.0);
    host.dcc.replay_decisions(DecisionTrace {
        epochs: (1..=5)
            .map(|epoch| record(allocation(epoch, Trigger::Refresh, Vec::new())))
            .collect(),
    });
    host.start();

    let replayed = host.frames_until(3, |dcc| !dcc.is_replaying());
    let lent = host.frames_until(2, |_| !ear.heard().lent.is_empty());
    let live = epochs(&host.dcc);
    host.dcc.stop();

    assert!(replayed, "the replay ran out");
    assert!(lent, "the agent is lent an envelope: {live:?}");
}

/// **A stall on a replayed epoch still being lent is not dropped.** A replay
/// publishes its epochs in turn and the scheduler takes one per frame start,
/// so while the DCC has already put epoch 43 in force, a long frame's
/// successors still run on 41. Those runs are not of an epoch the DCC
/// answered — 41 was never followed by anything the agent ran on — and a
/// stall the renderer reports on 41 raises `AgentStalled`, as it would on
/// the epoch the DCC holds in force. (The calls are the DCC loop's, in its
/// order: a live epoch, then three replayed ones.)
#[test]
fn a_stall_on_a_replayed_epoch_still_being_lent_is_not_dropped() {
    use super::super::epochs::Epochs;

    let (renderer, _ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    let agents = [renderer];
    let mut tracker = Epochs::default();
    tracker.observe(&agents);
    tracker.opened(&agents);
    tracker.set_in_force(Epoch(1));
    for replayed in 41..=43 {
        tracker.observe(&agents);
        tracker.replayed(&agents);
        tracker.set_in_force(Epoch(replayed));
    }

    // The frames have taken 41 only.
    tracker.report(
        &Report {
            epoch: Epoch(41),
            stalled: true,
            health: 0.2,
            ..renderer_report()
        },
        3,
    );

    assert_eq!(tracker.due(0), Some(Trigger::AgentStalled));
}
