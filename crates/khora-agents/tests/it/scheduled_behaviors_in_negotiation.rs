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

//! **GORNA hears the scheduling attributes** through the script agent: its
//! estimate weighs a rate-limited instance by the share of frames it runs
//! (`min(1, rate × frame delta)` of an instance); its cheapest strategy is
//! never priced below what the critical behaviors spent last frame; its
//! status names the critical instances a frame deferred; and a frame a
//! rate-limited instance skips is not a deferral, so it does not lower the
//! agent's health.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use khora_agents::script_agent::ScriptAgent;
use khora_core::agent::gorna::{AgentStatus, NegotiationRequest, ResourceConstraints, StrategyId};
use khora_core::agent::{Agent, EngineMode, ExecutionTiming};
use khora_core::lane::{LaneBus, OutputDeck};
use khora_core::{EngineContext, Runtime, WorldAccess};
use khora_data::ecs::{Transform, World};

use super::behavior_session::{attach, Project, Session, PLENTY};

/// A frame of 1/60 s.
const FRAME: f32 = 1.0 / 60.0;

const UNRATED: Project = &[(
    "game/tick.erg",
    "behavior Tick { int ticks = 0; void Update(float dt) { ticks += 1; } }",
)];

const TEN_HERTZ: Project = &[(
    "game/tick.erg",
    "[Rate(10hz)]
     behavior Tick { int ticks = 0; void Update(float dt) { ticks += 1; } }",
)];

const HUNDRED_TWENTY_HERTZ: Project = &[(
    "game/tick.erg",
    "[Rate(120hz)]
     behavior Tick { int ticks = 0; void Update(float dt) { ticks += 1; } }",
)];

/// A session over `count` entities holding `behavior`.
fn session_of(project: Project, behavior: &str, count: usize) -> Session {
    let mut session = Session::new(World::new(), project);
    for _ in 0..count {
        let entity = session.world.spawn(Transform::identity());
        attach(&mut session.world, entity, behavior, &[]);
    }
    session
}

/// `read` of a script agent handed the session's scripting world, as the
/// engine hands it; the world goes back to the session after.
fn with_agent<T>(session: &mut Session, read: impl FnOnce(&mut ScriptAgent) -> T) -> T {
    let shared = Arc::new(Mutex::new(std::mem::take(&mut session.scripts)));
    let answer = {
        let mut runtime = Runtime::default();
        runtime.services.insert(Arc::clone(&shared));
        let runtime = Arc::new(runtime);
        let mut agent = ScriptAgent::default();
        let bus = LaneBus::new();
        let mut deck = OutputDeck::new();
        let permit = agent.contention();
        {
            let mut ctx = EngineContext::for_agent(
                WorldAccess::None,
                Arc::clone(&runtime),
                &bus,
                &mut deck,
                &permit,
                Some(agent.id()),
            );
            agent.on_initialize(&mut ctx);
        }
        read(&mut agent)
    };
    session.scripts = Arc::try_unwrap(shared)
        .expect("the agent is gone, and its handle with it")
        .into_inner()
        .expect("the scripting world's lock is not poisoned");
    answer
}

/// The time the agent offers `strategy` for, negotiating for 1/60-s frames.
fn estimate(session: &mut Session, strategy: StrategyId) -> Duration {
    let response = with_agent(session, |agent| {
        agent.negotiate(NegotiationRequest {
            target_latency: Duration::from_secs_f64(f64::from(FRAME)),
            priority_weight: 1.0,
            constraints: ResourceConstraints::default(),
            current_mode: EngineMode::Playing,
            agent_timing: ExecutionTiming::default(),
        })
    });
    response
        .strategies
        .iter()
        .find(|option| option.id == strategy)
        .map(|option| option.estimated_time)
        .unwrap_or_else(|| panic!("a {strategy:?} offer"))
}

fn status(session: &mut Session) -> AgentStatus {
    with_agent(session, |agent| agent.report_status())
}

/// The Balanced estimate for sixty `Tick`s of `project` after three frames.
fn balanced_for_sixty(project: Project) -> Duration {
    let mut session = session_of(project, "Tick", 60);
    for _ in 0..3 {
        session.frame(FRAME);
    }
    estimate(&mut session, StrategyId::Balanced)
}

/// **A rate-limited instance is priced as the share of frames it runs.**
/// Sixty `Tick`s at 10 Hz in 1/60-s frames run a sixth of the frames each:
/// priced at most a third of sixty unrated ones. At 120 Hz — more than once
/// a frame — an instance counts as one, and the price is the unrated one.
#[test]
fn a_rate_limited_instance_is_priced_as_the_share_of_frames_it_runs() {
    let unrated = balanced_for_sixty(UNRATED);
    let ten_hertz = balanced_for_sixty(TEN_HERTZ);
    let fast = balanced_for_sixty(HUNDRED_TWENTY_HERTZ);

    assert!(
        ten_hertz * 3 <= unrated,
        "sixty at 10 Hz in 1/60-s frames are about ten instances' work: {ten_hertz:?} vs {unrated:?} unrated"
    );
    assert_eq!(
        fast, unrated,
        "a rate above the frame rate counts each instance once"
    );
}

/// `Heavy`, critical: a loop of three hundred a turn.
const CRITICAL_HEAVY: Project = &[(
    "game/heavy.erg",
    "[Critical]
     behavior Heavy { int ticks = 0;
     void Update(float dt) { int n = 0; while (n < 300) { n += 1; } ticks += 1; } }",
)];

/// The same `Heavy`, not critical.
const PLAIN_HEAVY: Project = &[(
    "game/heavy.erg",
    "behavior Heavy { int ticks = 0;
     void Update(float dt) { int n = 0; while (n < 300) { n += 1; } ticks += 1; } }",
)];

/// Four `Heavy`s of `project` after three frames with plenty of fuel, the
/// LowPower estimate, and what the frame's spending costs at the measured
/// rate.
fn low_power_and_last_cost(project: Project) -> (Duration, Duration) {
    let mut session = session_of(project, "Heavy", 4);
    for _ in 0..3 {
        session.frame(FRAME);
    }
    let spent = session.scripts.last_report().spent;
    let rate = session.scripts.rate();
    let cost = Duration::from_secs_f64(spent as f64 / rate / 1_000.0);
    (estimate(&mut session, StrategyId::LowPower), cost)
}

/// **The cheapest strategy covers what the critical behaviors spent.** Four
/// `[Critical]` `Heavy`s spend a few thousand instructions a frame; LowPower,
/// a quarter of an estimate that prices each instance at two microseconds, is
/// floored at that spending, converted at the measured rate. The same four
/// without `[Critical]` are not floored.
#[test]
fn low_power_is_floored_at_what_the_critical_behaviors_spent() {
    let (critical_low, critical_cost) = low_power_and_last_cost(CRITICAL_HEAVY);
    let (plain_low, plain_cost) = low_power_and_last_cost(PLAIN_HEAVY);

    assert!(
        critical_low.as_secs_f64() >= critical_cost.as_secs_f64() * 0.99,
        "LowPower {critical_low:?} covers the critical behaviors' {critical_cost:?}"
    );
    assert!(
        plain_low < plain_cost,
        "set-up: unfloored, LowPower {plain_low:?} is below what the frame spent ({plain_cost:?})"
    );
}

const VITAL_AND_PLAIN: Project = &[(
    "game/crew.erg",
    "[Critical]
     behavior Vital { int ticks = 0; void Update(float dt) { ticks += 1; } }
     behavior Plain { int ticks = 0; void Update(float dt) { ticks += 1; } }",
)];

/// **The status names the critical instances a frame deferred.** Seven
/// `Vital`s, critical, and four `Plain`s; a frame with no fuel defers all
/// eleven. The status message says how many of them were critical: seven.
#[test]
fn the_status_names_how_many_critical_instances_were_deferred() {
    let mut session = Session::new(World::new(), VITAL_AND_PLAIN);
    for (behavior, count) in [("Vital", 7), ("Plain", 4)] {
        for _ in 0..count {
            let entity = session.world.spawn(Transform::identity());
            attach(&mut session.world, entity, behavior, &[]);
        }
    }
    session.frame(FRAME);
    session.frame(FRAME);

    session.fuel = 0;
    session.frame(FRAME);
    session.fuel = PLENTY;
    assert_eq!(
        session.scripts.last_report().deferred,
        11,
        "set-up: {:?}",
        session.scripts.last_report()
    );

    let message = status(&mut session).message;
    let numbers: Vec<&str> = message
        .split(|c: char| !c.is_ascii_digit())
        .filter(|word| !word.is_empty())
        .collect();
    assert!(
        message.to_lowercase().contains("critical") && numbers.contains(&"7"),
        "the status names the seven critical instances deferred: {message:?}"
    );
}

/// **A frame a rate-limited instance skips does not lower the agent's
/// health.** Twelve `Tick`s at 10 Hz in 1/60-s frames with plenty of fuel:
/// most frames run only some of them, none is deferred, and every frame's
/// status reports full health.
#[test]
fn rate_skipped_frames_do_not_lower_the_script_agents_health() {
    let mut session = session_of(TEN_HERTZ, "Tick", 12);
    for _ in 0..12 {
        session.frame(FRAME);
    }

    let mut skipped_some = false;
    for at in 0..12 {
        session.frame(FRAME);
        let report = session.scripts.last_report().clone();
        assert_eq!(report.deferred, 0, "frame {at}: {report:?}");
        skipped_some |= report.completed < 12;
        let status = status(&mut session);
        assert_eq!(
            status.health_score, 1.0,
            "frame {at}: a skipped turn is not a deferral — {}",
            status.message
        );
    }
    assert!(
        skipped_some,
        "at 10 Hz in 1/60-s frames some frames skip some instances"
    );
}
