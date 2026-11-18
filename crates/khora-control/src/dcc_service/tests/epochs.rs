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

//! When the running DCC opens an epoch: a family switched on or off, a new agent, agents
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

/// A DCC holding a renderer and an audio agent, whose epochs may follow one
/// another every tick.
fn two_speakers(config: DccConfig) -> (DccService, Receiver<TelemetryEvent>, Ear, Ear) {
    let (dcc, rx) = recording(config);
    let (renderer, renderer_ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    let (audio, audio_ear) = Speaker::with_ear("audio", Duration::from_millis(1));
    dcc.register_agent(renderer, 1.0);
    dcc.register_agent(audio, 0.5);
    (dcc, rx, renderer_ear, audio_ear)
}

/// The families now off, as the frame boundary tells the DCC.
fn switched_off(names: &[&'static str]) -> TelemetryEvent {
    TelemetryEvent::ActivationChanged(names.iter().map(|name| AgentId::new(name)).collect())
}

/// Whether an `AgentsChanged` epoch opened that names no envelope for
/// `name`.
fn opened_without(dcc: &DccService, name: &'static str) -> bool {
    epochs(dcc).iter().any(|epoch| {
        epoch.trigger == Trigger::AgentsChanged
            && epoch
                .envelopes
                .iter()
                .all(|(id, _)| *id != AgentId::new(name))
    })
}

/// **A family switched off opens an epoch**: the agents taking part changed,
/// so an `AgentsChanged` epoch opens, lending nothing to the family off.
#[test]
fn a_family_switched_off_opens_an_epoch_without_it() {
    let (mut dcc, rx, _renderer, _audio) = two_speakers(quick());
    let tx = dcc.event_sender();
    dcc.start(rx);
    assert!(
        wait_for(3, || opened_by(&dcc, Trigger::Initial)),
        "the first epoch"
    );

    tx.send(switched_off(&["audio"])).expect("the DCC listens");
    let opened = wait_for(3, || opened_without(&dcc, "audio"));
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
        message: None,
    }
}

/// The epoch the first allocation opened, once it has.
fn first_epoch(dcc: &DccService) -> Epoch {
    assert!(wait_for(3, || opened_by(dcc, Trigger::Initial)));
    epochs(dcc)[0].epoch
}

/// **Over-budget reports in a row open an `Overruns` epoch.**
///
/// It opens once as many as the configuration says are published: the
/// reports the scheduler publishes, not ones the DCC asks the agent for
/// (asked, this agent says it is within budget).
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
///
/// Against a limit of five: four reports over budget, one within, four over —
/// eight over, never five in a row.
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
///
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

/// **Reports of epoch zero count toward overruns.**
///
/// An agent that reports before it was lent any epoch says `Epoch(0)`, whatever
/// epoch is in force; its over-budget reports in a row are not dropped as
/// stale, and open an `Overruns` epoch like any other agent's.
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

/// **The overruns an epoch answered open no other.**
///
/// Three over-budget reports on the first epoch open an `Overruns` epoch. The
/// scheduler lends it only at its next frame start, so the runs of the frames
/// still going while the round decided — over budget, on the first epoch —
/// reach the DCC after the epoch opened (a physics agent sub-stepping three
/// times a frame sends three in one frame). Then the agent, lent the new epoch,
/// runs within it. The overruns were already answered: no second `Overruns`
/// epoch opens.
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

/// **An epoch lasts at least its minimum.**
///
/// With a minimum of 30 ticks (300 ms at 100 Hz), a family switched off right
/// after the first epoch opens no epoch at once; it is not lost either: its
/// epoch opens once the minimum has passed.
#[test]
fn an_epoch_lasts_at_least_its_minimum_ticks() {
    let (mut dcc, rx, _renderer, _audio) = two_speakers(DccConfig {
        min_epoch_ticks: 30,
        ..Default::default()
    });
    let tx = dcc.event_sender();
    dcc.start(rx);
    assert!(wait_for(3, || opened_by(&dcc, Trigger::Initial)));

    tx.send(switched_off(&["audio"])).expect("the DCC listens");
    thread::sleep(Duration::from_millis(120));
    let too_soon = epochs(&dcc);
    let later = wait_for(5, || opened_without(&dcc, "audio"));
    dcc.stop();

    assert_eq!(
        too_soon.len(),
        1,
        "no second epoch within 12 ticks: {too_soon:?}"
    );
    assert!(later, "the switch opens its epoch after the minimum");
}

/// **`Safety` does not wait for the epoch to end.**
///
/// Under a minimum of 100 000 ticks, a death spiral (critical heat, critical
/// battery, a saturated processor) opens a `Safety` epoch at once, lending the
/// agent its floor.
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

/// **A stalled, unhealthy agent opens an `AgentStalled` epoch.**
///
/// Learnt from its published report (asked, this agent says it is well).
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

/// **An agent busy when an epoch opens does not reopen it.**
///
/// The set of agents does not change; one of them is running (its lock held) at
/// the moment the epoch opens. The next tick reads every agent and finds the
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

/// **A family switched off during a replay opens an epoch once the replay
/// ends.**
///
/// The replay lends recorded allocations for 20 ticks; a family is switched off
/// meanwhile. The change is not dropped: when live arbitration resumes, an
/// `AgentsChanged` epoch opens for the families on.
#[test]
fn a_family_switched_off_during_a_replay_opens_an_epoch_after_it() {
    let (mut dcc, rx, _renderer, _audio) = two_speakers(quick());
    dcc.replay_decisions(DecisionTrace {
        epochs: (1..=20)
            .map(|epoch| record(allocation(epoch, Trigger::Refresh, Vec::new())))
            .collect(),
    });
    let tx = dcc.event_sender();
    dcc.start(rx);
    tx.send(switched_off(&["audio"])).expect("the DCC listens");

    let replayed = wait_for(3, || !dcc.is_replaying());
    let opened = wait_for(2, || opened_without(&dcc, "audio"));
    let live = epochs(&dcc);
    dcc.stop();

    assert!(replayed, "the replay ran out");
    assert!(opened, "the switch opened its epoch: {live:?}");
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

/// **An agent a replay did not name is lent an envelope after it.**
///
/// The recorded allocations name no agent; once the replay is over, live
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

/// **A stall on a replayed epoch still being lent is not dropped.**
///
/// A replay publishes its epochs in turn and the scheduler takes one per frame
/// start, so while the DCC has already put epoch 43 in force, a long frame's
/// successors still run on 41. Those runs are not of an epoch the DCC answered
/// — 41 was never followed by anything the agent ran on — and a stall the
/// renderer reports on 41 raises `AgentStalled`, as it would on the epoch the
/// DCC holds in force. (The calls are the DCC loop's, in its order: a live
/// epoch, then three replayed ones.)
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

// ─── A developer's policy ─────────────────────────────────────────────────

/// A DCC whose epochs may follow one another every tick and that never
/// refreshes, holding a renderer offering a 2 ms floor and a 4 ms upgrade.
fn upgradable() -> (DccService, Receiver<TelemetryEvent>) {
    let (dcc, rx) = recording(DccConfig {
        refresh_every_ticks: 0,
        ..quick()
    });
    let (agent, _ear) = Speaker::with_upgrade(
        "renderer",
        Duration::from_millis(2),
        Duration::from_millis(4),
    );
    dcc.register_agent(agent, 1.0);
    (dcc, rx)
}

/// The renderer's host time in `epoch`'s envelope.
fn renderer_time(epoch: &Allocation) -> Option<Duration> {
    (epoch.envelopes.iter())
        .find(|(id, _)| *id == AgentId::new("renderer"))
        .map(|(_, envelope)| envelope.time.cpu)
}

/// **A mode set on an agent changes its allocation, with no other trigger.**
///
/// Nothing else moves: no frame, no heat, no refresh. Pinning the renderer to
/// its floor opens a `PolicyChanged` epoch within a few ticks, which lends the
/// pin.
#[test]
fn a_mode_set_on_an_agent_opens_an_epoch_that_lends_it() {
    let (mut dcc, rx) = upgradable();
    dcc.start(rx);
    assert!(wait_for(3, || opened_by(&dcc, Trigger::Initial)));

    dcc.set_adaptation_mode(
        AgentId::new("renderer"),
        AdaptationMode::Manual(StrategyId::LowPower),
    );
    let opened = wait_for(3, || opened_by(&dcc, Trigger::PolicyChanged));
    let recorded = epochs(&dcc);
    dcc.stop();

    assert!(opened, "the mode opened its epoch: {recorded:?}");
    let changed = (recorded.iter())
        .find(|epoch| epoch.trigger == Trigger::PolicyChanged)
        .expect("the PolicyChanged epoch");
    assert_eq!(
        changed.pins,
        vec![(AgentId::new("renderer"), StrategyId::LowPower)],
        "the pin is lent"
    );
}

/// **A hint set on an agent changes its allocation, with no other trigger.**
///
/// The first epoch lends the renderer its 4 ms upgrade; a cap of 3 ms opens a
/// `PolicyChanged` epoch whose envelope stays under it.
#[test]
fn a_hint_set_on_an_agent_opens_an_epoch_that_lends_it() {
    let (mut dcc, rx) = upgradable();
    dcc.start(rx);
    assert!(wait_for(3, || opened_by(&dcc, Trigger::Initial)));
    let first = renderer_time(&epochs(&dcc)[0]).expect("the renderer's envelope");

    dcc.set_hint(EngineHint::Cap {
        agent: AgentId::new("renderer"),
        max_ms: 3.0,
    });
    let opened = wait_for(3, || opened_by(&dcc, Trigger::PolicyChanged));
    let recorded = epochs(&dcc);
    dcc.stop();

    assert!(opened, "the hint opened its epoch: {recorded:?}");
    assert!(first >= Duration::from_millis(4), "uncapped: {first:?}");
    let capped = (recorded.iter())
        .find(|epoch| epoch.trigger == Trigger::PolicyChanged)
        .and_then(renderer_time)
        .expect("the renderer's envelope");
    assert!(capped <= Duration::from_millis(3), "capped: {capped:?}");
}

/// **Clearing an agent's hints opens an epoch.** Taking the cap back is a
/// change of policy too.
#[test]
fn clearing_an_agents_hints_opens_an_epoch() {
    let (mut dcc, rx) = upgradable();
    dcc.set_hint(EngineHint::Cap {
        agent: AgentId::new("renderer"),
        max_ms: 3.0,
    });
    dcc.start(rx);
    assert!(wait_for(3, || opened_by(&dcc, Trigger::Initial)));

    dcc.clear_agent_hints(AgentId::new("renderer"));
    let opened = wait_for(3, || opened_by(&dcc, Trigger::PolicyChanged));
    let recorded = epochs(&dcc);
    dcc.stop();

    assert!(opened, "{recorded:?}");
}

/// **The same policy set again opens no epoch.** The round reads the
/// developer's rules; rules it already read change nothing.
#[test]
fn the_same_policy_set_again_opens_no_epoch() {
    let (mut dcc, rx) = upgradable();
    dcc.start(rx);
    assert!(wait_for(3, || opened_by(&dcc, Trigger::Initial)));
    let pin = AdaptationMode::Manual(StrategyId::LowPower);

    dcc.set_adaptation_mode(AgentId::new("renderer"), pin);
    assert!(wait_for(3, || opened_by(&dcc, Trigger::PolicyChanged)));
    dcc.set_adaptation_mode(AgentId::new("renderer"), pin);
    thread::sleep(Duration::from_millis(300));
    let recorded = epochs(&dcc);
    dcc.stop();

    let changed = (recorded.iter())
        .filter(|epoch| epoch.trigger == Trigger::PolicyChanged)
        .count();
    assert_eq!(changed, 1, "{recorded:?}");
}

/// **A policy change waits for the epoch's minimum.**
///
/// Under a minimum of 30 ticks (300 ms at 100 Hz), a mode set right after
/// the first epoch opens no epoch within 12 ticks; it opens its epoch once the
/// minimum has passed. Only `Safety` preempts the minimum.
#[test]
fn a_policy_change_waits_for_the_epochs_minimum() {
    let (mut dcc, rx) = recording(DccConfig {
        min_epoch_ticks: 30,
        refresh_every_ticks: 0,
        ..Default::default()
    });
    let (agent, _ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    dcc.register_agent(agent, 1.0);
    dcc.start(rx);
    assert!(wait_for(3, || opened_by(&dcc, Trigger::Initial)));

    dcc.set_adaptation_mode(AgentId::new("renderer"), AdaptationMode::Stable);
    thread::sleep(Duration::from_millis(120));
    let too_soon = epochs(&dcc);
    let later = wait_for(5, || opened_by(&dcc, Trigger::PolicyChanged));
    dcc.stop();

    assert_eq!(
        too_soon.len(),
        1,
        "no second epoch within 12 ticks: {too_soon:?}"
    );
    assert!(later, "the change opens its epoch after the minimum");
}

/// **Setting the mode an agent already runs opens no epoch.**
///
/// No mode was set for the renderer, so it runs `Learning`, the default:
/// setting `Learning` sets the rule already in force, and a round would read
/// the same rules as the last one.
#[test]
fn setting_the_default_mode_opens_no_epoch() {
    let (mut dcc, rx) = upgradable();
    dcc.start(rx);
    assert!(wait_for(3, || opened_by(&dcc, Trigger::Initial)));

    dcc.set_adaptation_mode(AgentId::new("renderer"), AdaptationMode::Learning);
    thread::sleep(Duration::from_millis(300));
    let recorded = epochs(&dcc);
    dcc.stop();

    assert!(
        (recorded.iter()).all(|epoch| epoch.trigger != Trigger::PolicyChanged),
        "the rule already in force: {recorded:?}"
    );
}

/// **A hint set and cleared before its epoch opens opens none.**
///
/// Under a minimum of 30 ticks (300 ms at 100 Hz), a cap is set on the
/// renderer, seen for a few ticks, then cleared: by the time an epoch could
/// open, the rules are those the last round read, and none opens.
#[test]
fn a_hint_set_and_cleared_before_its_epoch_opens_none() {
    let (mut dcc, rx) = recording(DccConfig {
        min_epoch_ticks: 30,
        refresh_every_ticks: 0,
        ..Default::default()
    });
    let (agent, _ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    dcc.register_agent(agent, 1.0);
    dcc.start(rx);
    assert!(wait_for(3, || opened_by(&dcc, Trigger::Initial)));

    dcc.set_hint(EngineHint::Cap {
        agent: AgentId::new("renderer"),
        max_ms: 3.0,
    });
    thread::sleep(Duration::from_millis(50));
    dcc.clear_agent_hints(AgentId::new("renderer"));
    thread::sleep(Duration::from_millis(600));
    let recorded = epochs(&dcc);
    dcc.stop();

    assert!(
        (recorded.iter()).all(|epoch| epoch.trigger != Trigger::PolicyChanged),
        "the rules are back to those the last round read: {recorded:?}"
    );
}

// ─── The periodic refresh ─────────────────────────────────────────────────

/// **A quiet engine opens a `Refresh` epoch once the period has passed.**
///
/// Nothing opens a round after the first: after 20 ticks with none, the DCC
/// opens one as `Refresh`, so what it learnt reaches the plan.
#[test]
fn a_quiet_engine_refreshes_after_the_period() {
    let (mut dcc, rx, _ear) = one_speaker(DccConfig {
        refresh_every_ticks: 20,
        ..quick()
    });
    dcc.start(rx);
    assert!(wait_for(3, || opened_by(&dcc, Trigger::Initial)));

    let refreshed = wait_for(3, || opened_by(&dcc, Trigger::Refresh));
    let recorded = records(&dcc);
    dcc.stop();

    assert!(refreshed, "{:?}", triggers(&recorded));
    let initial = &recorded[0];
    let refresh = (recorded.iter())
        .find(|record| record.allocation.trigger == Trigger::Refresh)
        .expect("the Refresh epoch");
    assert!(
        refresh.at - initial.at >= Duration::from_millis(180),
        "not before 20 ticks: {:?} after the first",
        refresh.at - initial.at
    );
}

/// **A round opened by another trigger restarts the period.**
///
/// With a period of 50 ticks (500 ms at 100 Hz), a family switched off 300 ms
/// after the first epoch opens its own; the refresh comes 50 ticks after
/// that one, not 50 after the first.
#[test]
fn a_round_opened_by_another_trigger_restarts_the_period() {
    let (mut dcc, rx, _renderer, _audio) = two_speakers(DccConfig {
        refresh_every_ticks: 50,
        ..quick()
    });
    let tx = dcc.event_sender();
    dcc.start(rx);
    assert!(wait_for(3, || opened_by(&dcc, Trigger::Initial)));

    thread::sleep(Duration::from_millis(300));
    tx.send(switched_off(&["audio"])).expect("the DCC listens");
    let refreshed = wait_for(5, || opened_by(&dcc, Trigger::Refresh));
    let recorded = records(&dcc);
    dcc.stop();

    assert!(refreshed, "{:?}", triggers(&recorded));
    let at = |trigger| {
        (recorded.iter())
            .find(|record| record.allocation.trigger == trigger)
            .map(|record| record.at)
            .expect("the epoch")
    };
    let since_switch = at(Trigger::Refresh).saturating_sub(at(Trigger::AgentsChanged));
    assert!(
        at(Trigger::Refresh) > at(Trigger::AgentsChanged)
            && since_switch >= Duration::from_millis(450),
        "the refresh counts from the last round: {since_switch:?} after it, {:?}",
        triggers(&recorded)
    );
}

/// **An allocation not yet lent holds the refresh.**
///
/// The round that published it stays open until a frame start lends it: with
/// no frame running, no `Refresh` opens, however long the engine stays quiet.
/// Once frames run, it does.
#[test]
fn an_allocation_not_yet_lent_holds_the_refresh() {
    let mut host = Host::new(DccConfig {
        tick_rate: 100,
        refresh_every_ticks: 10,
        ..quick()
    });
    let (agent, _ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    host.dcc.register_agent(agent, 1.0);
    host.start();
    assert!(wait_for(3, || opened_by(&host.dcc, Trigger::Initial)));

    thread::sleep(Duration::from_millis(300));
    let unlent = epochs(&host.dcc);
    let refreshed = host.frames_until(3, |dcc| opened_by(dcc, Trigger::Refresh));
    host.dcc.stop();

    assert!(
        unlent.iter().all(|epoch| epoch.trigger != Trigger::Refresh),
        "no frame start lent the first epoch: {unlent:?}"
    );
    assert!(refreshed, "frames lend it, and the refresh follows");
}

/// **A withdrawn trigger leaves the one raised behind it.**
///
/// A policy change waits; agents report overruns meanwhile. The change is put
/// back, its trigger withdrawn: the overruns still open their epoch.
#[test]
fn a_withdrawn_trigger_leaves_the_one_raised_behind_it() {
    use super::super::epochs::Epochs;

    let (renderer, _ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    let agents = [renderer];
    let mut tracker = Epochs::default();
    tracker.observe(&agents);
    tracker.opened(&agents);
    tracker.raise(Trigger::PolicyChanged);
    tracker.raise(Trigger::Overruns);

    tracker.withdraw(Trigger::PolicyChanged);

    assert_eq!(tracker.due(0), Some(Trigger::Overruns));
}

/// **A withdrawn trigger with none behind it leaves nothing waiting.**
#[test]
fn a_withdrawn_trigger_alone_leaves_nothing_waiting() {
    use super::super::epochs::Epochs;

    let (renderer, _ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    let agents = [renderer];
    let mut tracker = Epochs::default();
    tracker.observe(&agents);
    tracker.opened(&agents);
    tracker.raise(Trigger::PolicyChanged);
    tracker.raise(Trigger::PolicyChanged);

    tracker.withdraw(Trigger::PolicyChanged);

    assert_eq!(tracker.due(0), None);
}

/// **No refresh while a trigger waits.** A trigger raised during the epoch
/// waits for its minimum and opens the next epoch under its own name; the
/// period passing meanwhile does not rename it.
#[test]
fn no_refresh_while_a_trigger_waits() {
    use super::super::epochs::Epochs;

    let (renderer, _ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    let agents = [renderer];
    let mut tracker = Epochs::default();
    tracker.observe(&agents);
    tracker.opened(&agents);
    tracker.report(
        &Report {
            stalled: true,
            health: 0.2,
            ..renderer_report()
        },
        3,
    );
    for _ in 0..10 {
        tracker.observe(&agents);
        tracker.refresh(5);
    }

    assert_eq!(tracker.due(3), Some(Trigger::AgentStalled));
}

/// **A refresh period of zero never refreshes.**
#[test]
fn a_refresh_period_of_zero_never_refreshes() {
    use super::super::epochs::Epochs;

    let (renderer, _ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    let agents = [renderer];
    let mut tracker = Epochs::default();
    tracker.observe(&agents);
    tracker.opened(&agents);
    for _ in 0..1_000 {
        tracker.observe(&agents);
        tracker.refresh(0);
    }

    assert_eq!(tracker.due(0), None);
}

/// **A quiet epoch refreshes once its period has passed, not before.**
#[test]
fn a_quiet_epoch_refreshes_once_its_period_has_passed() {
    use super::super::epochs::Epochs;

    let (renderer, _ear) = Speaker::with_ear("renderer", Duration::from_millis(2));
    let agents = [renderer];
    let mut tracker = Epochs::default();
    tracker.observe(&agents);
    tracker.opened(&agents);
    for _ in 0..4 {
        tracker.observe(&agents);
        tracker.refresh(5);
    }
    let before = tracker.due(0);
    tracker.observe(&agents);
    tracker.refresh(5);

    assert_eq!(before, None, "4 ticks of 5");
    assert_eq!(tracker.due(0), Some(Trigger::Refresh));
}

/// The trigger of each journaled epoch, in order.
fn triggers(recorded: &[EpochRecord]) -> Vec<Trigger> {
    (recorded.iter())
        .map(|record| record.allocation.trigger)
        .collect()
}
