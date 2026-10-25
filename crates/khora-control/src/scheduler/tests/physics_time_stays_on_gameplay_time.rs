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

//! **Physics time stays on gameplay time at the edges.** The gameplay tick
//! ([`DEFAULT_FIXED_DELTA_SECONDS`]) is the game's clock; a fixed agent's
//! declared step only cuts the time the ticks advanced. These tests push that
//! sub-clock where rounding, scale and odd steps live: a step declared in
//! whole microseconds, an hour of play, the step switched every frame, the
//! time scale from slow motion to infinity, a pause with a step change under
//! it, a step longer than the catch-up cap, a zero step, two fixed agents.
//!
//! Gameplay time is always read back from what the scheduler published —
//! the sum of `Time::fixed_steps` × the tick — so the frames' wall-clock
//! jitter never enters the comparison.

use crate::agent_registry::AgentRegistry;
use crate::dcc_context::Context;
use crate::scheduler::sim_steps::{compute_sim_steps, compute_sub_steps};
use crate::scheduler::*;
use khora_core::agent::gorna::{
    AgentId, AgentStatus, NegotiationRequest, NegotiationResponse, ResourceBudget, StrategyId,
};
use khora_core::agent::{Agent, ExecutionPhase, ExecutionTiming};
use khora_core::time::{SharedTime, Time, DEFAULT_FIXED_DELTA_SECONDS, MAX_SIM_STEPS};
use khora_core::{EngineContext, Runtime};
use khora_data::ecs::World;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

/// The gameplay tick, in seconds.
const TICK: f64 = DEFAULT_FIXED_DELTA_SECONDS as f64;

/// The physics steps GORNA chooses between, in hertz.
const GORNA_RATES: [f32; 3] = [30.0, 60.0, 120.0];

/// A GORNA step as the scheduler sees it: the agent's `f32` step, through the
/// `Duration` it declares.
fn gorna_step(hz: f32) -> Duration {
    Duration::from_secs_f32(1.0 / hz)
}

/// What an integrating agent has seen.
#[derive(Debug, Default, Clone, Copy)]
struct Seen {
    simulated: f64,
    runs: usize,
}

/// Declares the step in `step` as its fixed timestep (none when `None`) and
/// adds that step to the simulated time it has seen on each run.
struct Integrating {
    id: AgentId,
    step: Arc<Mutex<Option<Duration>>>,
    seen: Arc<Mutex<Seen>>,
}

impl Agent for Integrating {
    fn id(&self) -> AgentId {
        self.id
    }
    fn negotiate(&mut self, _r: NegotiationRequest) -> NegotiationResponse {
        NegotiationResponse {
            strategies: vec![],
            timing_adjustment: None,
        }
    }
    fn apply_budget(&mut self, _b: ResourceBudget) {}
    fn report_status(&self) -> AgentStatus {
        AgentStatus {
            agent_id: self.id,
            current_strategy: StrategyId::Balanced,
            health_score: 1.0,
            is_stalled: false,
            message: String::new(),
        }
    }
    fn execute(&mut self, _ctx: &mut EngineContext<'_>) {
        let step = self
            .step
            .lock()
            .expect("the step lock")
            .map_or(0.0, |d| d.as_secs_f64());
        let mut seen = self.seen.lock().expect("the seen lock");
        seen.simulated += step;
        seen.runs += 1;
    }
    fn execution_timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            allowed_phases: vec![ExecutionPhase::TRANSFORM],
            default_phase: ExecutionPhase::TRANSFORM,
            fixed_timestep: *self.step.lock().expect("the step lock"),
            ..ExecutionTiming::default()
        }
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// One registered agent's handles.
struct Handle {
    step: Arc<Mutex<Option<Duration>>>,
    seen: Arc<Mutex<Seen>>,
}

impl Handle {
    fn seen(&self) -> Seen {
        *self.seen.lock().expect("the seen lock")
    }
    fn switch_to(&self, step: Duration) {
        *self.step.lock().expect("the step lock") = Some(step);
    }
}

/// A scheduler running one `Integrating` agent per `(id, step)`, the world,
/// the runtime, its clock, and the gameplay time it has published so far.
struct Rig {
    scheduler: ExecutionScheduler,
    agents: Vec<Handle>,
    world: World,
    runtime: Arc<Runtime>,
    time: SharedTime,
    gameplay: f64,
}

impl Rig {
    fn new(agents: &[(AgentId, Option<Duration>)], scale: f32) -> Self {
        let registry = Arc::new(Mutex::new(AgentRegistry::new()));
        let mut handles = Vec::new();
        for &(id, step) in agents {
            let step = Arc::new(Mutex::new(step));
            let seen = Arc::new(Mutex::new(Seen::default()));
            registry.lock().expect("the registry lock").register(
                Arc::new(Mutex::new(Integrating {
                    id,
                    step: Arc::clone(&step),
                    seen: Arc::clone(&seen),
                })),
                1.0,
            );
            handles.push(Handle { step, seen });
        }
        let ids: Vec<AgentId> = agents.iter().map(|(id, _)| *id).collect();
        let scheduler =
            ExecutionScheduler::new(registry, Arc::new(RwLock::new(Context::default())), &ids);
        let mut clock = Time::default();
        clock.set_scale(scale);
        let time: SharedTime = Arc::new(RwLock::new(clock));
        let mut runtime = Runtime::new();
        runtime.resources.insert(Arc::clone(&time));
        Self {
            scheduler,
            agents: handles,
            world: World::new(),
            runtime: Arc::new(runtime),
            time,
            gameplay: 0.0,
        }
    }

    /// Runs one frame lasting `seconds` of wall clock; the frame's `Time`.
    fn frame_of(&mut self, seconds: f64) -> Time {
        let length = Duration::from_secs_f64(seconds) + Duration::from_micros(20);
        self.scheduler.last_frame_instant = Some(Instant::now() - length);
        self.scheduler
            .run_frame(&mut self.world, Arc::clone(&self.runtime));
        let time = *self.time.read().expect("the clock lock");
        self.gameplay += f64::from(time.fixed_steps) * TICK;
        time
    }

    fn set_scale(&self, scale: f32) {
        self.time.write().expect("the clock lock").set_scale(scale);
    }
}

/// **A step declared a hair over the tick does not run physics ahead of
/// gameplay.** A step written in whole microseconds — `16 667 µs` for 60 Hz,
/// `8 334 µs` for 120 Hz — is a few hundred nanoseconds longer than the tick
/// or its half. Fed 1/20-s frames (three ticks each) for a quarter of an
/// hour, physics must stay within one step of the gameplay time published.
///
/// The sub-clock's float slack counts a step as covered when the time owed
/// falls up to a thousandth of a step short, and the shortfall is then
/// clamped away instead of carried: every frame credits physics with time
/// the game's clock never passed, and the lead grows without bound.
#[test]
fn a_step_a_hair_over_the_tick_does_not_run_physics_ahead_of_gameplay() {
    for step in [Duration::from_micros(16_667), Duration::from_micros(8_334)] {
        let p = step.as_secs_f64();
        let mut rig = Rig::new(&[(AgentId::Physics, Some(step))], 1.0);
        for frame in 0..18_000 {
            rig.frame_of(0.05);
            let physics = rig.agents[0].seen().simulated;
            let ahead = physics - rig.gameplay;
            assert!(
                ahead.abs() < p,
                "a {} µs step, frame {frame} ({:.1} s of play): {physics} s of physics for \
                 {} s of gameplay — {:.2} steps apart",
                step.as_micros(),
                rig.gameplay,
                rig.gameplay,
                ahead / p
            );
        }
    }
}

/// **An hour of irregular frames with the step switched every frame never
/// drifts.** The scheduler's two clocks, driven directly for an hour of
/// frames from 2 ms to 42 ms, the GORNA step (as the scheduler sees it)
/// picked anew each frame: physics stays within the coarsest step, 1/30 s,
/// of gameplay time after every frame — `f32` rounding over 200 000 ticks
/// does not accumulate. Coverage: holds today.
#[test]
fn an_hour_of_irregular_frames_with_the_step_switched_every_frame_never_drifts() {
    let tick = DEFAULT_FIXED_DELTA_SECONDS;
    let steps: Vec<f32> = GORNA_RATES
        .iter()
        .map(|&hz| gorna_step(hz).as_secs_f32())
        .collect();
    let coarsest = f64::from(steps[0]);
    // A fixed xorshift so the hour is the same every run.
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f64 / (1u64 << 53) as f64
    };

    let (mut game_acc, mut physics_acc) = (0.0f32, 0.0f32);
    let (mut gameplay, mut physics, mut wall) = (0.0f64, 0.0f64, 0.0f64);
    let mut frame = 0u64;
    while wall < 3600.0 {
        let dt = (next() * 0.04 + 0.002) as f32;
        wall += f64::from(dt);
        let step = steps[(next() * 3.0) as usize];

        let ticks = compute_sim_steps(game_acc, dt, tick, MAX_SIM_STEPS);
        game_acc = ticks.new_accumulator;
        gameplay += f64::from(ticks.steps) * f64::from(tick);
        if ticks.steps > 0 {
            let sub = compute_sub_steps(physics_acc, ticks.steps as f32 * tick, step);
            physics_acc = sub.new_accumulator;
            physics += f64::from(sub.steps) * f64::from(step);
        }

        assert!(
            (physics - gameplay).abs() < coarsest,
            "frame {frame} ({wall:.1} s): {physics} s of physics for {gameplay} s of gameplay"
        );
        frame += 1;
    }
}

/// **Slow motion and fast forward keep physics on gameplay time under every
/// step.** The scale runs through 0.1, 0.5, 1, 3 and 10 over 1/30-s frames —
/// fast forward hitting the catch-up cap — and after every frame the physics
/// time is within one step of the gameplay ticks published. Coverage: holds
/// today.
#[test]
fn slow_motion_and_fast_forward_keep_physics_on_gameplay_time_under_every_step() {
    let scales = [0.1, 0.5, 1.0, 3.0, 10.0, 0.25, 7.0, 1.0];
    for hz in GORNA_RATES {
        let step = gorna_step(hz);
        let p = step.as_secs_f64();
        let mut rig = Rig::new(&[(AgentId::Physics, Some(step))], 1.0);
        for (i, &scale) in scales.iter().cycle().take(400).enumerate() {
            rig.set_scale(scale);
            let time = rig.frame_of(1.0 / 30.0);
            assert!(time.fixed_steps <= MAX_SIM_STEPS);
            let physics = rig.agents[0].seen().simulated;
            assert!(
                (physics - rig.gameplay).abs() < p,
                "1/{hz} s a step, frame {i} at scale {scale}: {physics} s of physics for \
                 {} s of gameplay",
                rig.gameplay
            );
        }
    }
}

/// **An infinite time scale does not stop the game's clock.** A scale is
/// clamped below at zero but not above; one frame at `f32::INFINITY` (a
/// scale computed as `1 / slowdown` with the slowdown at zero) must not
/// leave the gameplay accumulator unusable: back at scale 1, a 1/20-s frame
/// takes its three ticks and publishes a finite interpolation factor.
///
/// Not introduced by the two-clock change — the gameplay accumulator's
/// overflow arithmetic is the one the single clock had.
#[test]
fn an_infinite_time_scale_does_not_stop_the_games_clock() {
    let mut rig = Rig::new(&[(AgentId::Physics, Some(gorna_step(60.0)))], 1.0);
    rig.frame_of(0.05);

    rig.set_scale(f32::INFINITY);
    let fast = rig.frame_of(0.05);
    assert_eq!(fast.fixed_steps, MAX_SIM_STEPS, "infinitely fast: the cap");

    rig.set_scale(1.0);
    let after: Vec<(u32, f32)> = (0..4)
        .map(|_| {
            let time = rig.frame_of(0.05);
            (time.fixed_steps, time.interpolation_alpha)
        })
        .collect();
    assert!(
        after
            .iter()
            .all(|&(ticks, alpha)| ticks == 3 && alpha.is_finite()),
        "back at scale 1, each 1/20-s frame is three ticks with a finite alpha: {after:?}"
    );
}

/// **Paused, a step change runs nothing; resumed, the carried time is paid
/// at the new step.** At 1/30 s a step, a one-tick frame carries the tick;
/// paused, the step drops to 1/120 s — enough carried time for two steps —
/// and still no step runs in eight frames; resumed, a one-tick frame pays
/// both ticks at the new step: four runs, physics on gameplay time.
/// Coverage: holds today.
#[test]
fn paused_a_step_change_runs_nothing_and_resumed_the_carry_is_paid_at_the_new_step() {
    let mut rig = Rig::new(&[(AgentId::Physics, Some(gorna_step(30.0)))], 1.0);
    let first = rig.frame_of(TICK);
    assert_eq!(first.fixed_steps, 1, "set-up: one tick");
    assert_eq!(rig.agents[0].seen().runs, 0, "set-up: the tick carried");

    rig.set_scale(0.0);
    rig.agents[0].switch_to(gorna_step(120.0));
    for _ in 0..8 {
        let time = rig.frame_of(1.0 / 30.0);
        assert_eq!(time.fixed_steps, 0, "paused: no tick");
    }
    assert_eq!(
        rig.agents[0].seen().runs,
        0,
        "paused: the carried tick is not stepped under the new step"
    );

    rig.set_scale(1.0);
    let resumed = rig.frame_of(TICK);
    assert_eq!(resumed.fixed_steps, 1);
    let seen = rig.agents[0].seen();
    assert_eq!(seen.runs, 4, "two ticks of time at 1/120 s a step");
    assert!(
        (seen.simulated - rig.gameplay).abs() < 1e-6,
        "{} s of physics for {} s of gameplay",
        seen.simulated,
        rig.gameplay
    );
}

/// **A step longer than the catch-up cap still keeps physics within a
/// step.** At 0.1 s and 0.5 s a step — longer than the cap's 5/60 s — frames
/// from a quarter tick to a clamped quarter second run the agent only when a
/// whole step has passed on the gameplay clock, and the physics time is
/// never more than a step from it. Coverage: holds today.
#[test]
fn a_step_longer_than_the_catch_up_cap_keeps_physics_within_a_step() {
    let frames = [0.25, 1.0, 3.0, 15.0, 2.5, 18.0, 0.5, 6.0, 1.5, 4.0];
    for step in [Duration::from_millis(100), Duration::from_millis(500)] {
        let p = step.as_secs_f64();
        let mut rig = Rig::new(&[(AgentId::Physics, Some(step))], 1.0);
        for (i, &ticks) in frames.iter().cycle().take(300).enumerate() {
            rig.frame_of(ticks * TICK);
            let physics = rig.agents[0].seen().simulated;
            assert!(
                physics <= rig.gameplay + 1e-6 && rig.gameplay - physics < p,
                "a {} ms step, frame {i}: {physics} s of physics for {} s of gameplay",
                step.as_millis(),
                rig.gameplay
            );
        }
        assert!(rig.agents[0].seen().runs > 0, "set-up: the agent ran");
    }
}

/// **A zero fixed step is no fixed step.** An agent declaring
/// `Some(Duration::ZERO)` runs once a frame, alone or beside a real fixed
/// agent, and the game's clock still ticks: a 1/20-s frame publishes three
/// ticks while the 1/120-s agent beside it sub-steps six times. Coverage:
/// holds today.
#[test]
fn a_zero_fixed_step_is_no_fixed_step() {
    let mut alone = Rig::new(&[(AgentId::Script, Some(Duration::ZERO))], 1.0);
    for frame in 1..=4 {
        let time = alone.frame_of(0.05);
        assert_eq!(time.fixed_steps, 3, "frame {frame}: three ticks");
        assert_eq!(alone.agents[0].seen().runs, frame, "once a frame");
    }

    let mut beside = Rig::new(
        &[
            (AgentId::Script, Some(Duration::ZERO)),
            (AgentId::Physics, Some(gorna_step(120.0))),
        ],
        1.0,
    );
    for frame in 1..=4 {
        let time = beside.frame_of(0.05);
        assert_eq!(time.fixed_steps, 3, "frame {frame}: three ticks");
        assert_eq!(
            beside.agents[0].seen().runs,
            frame,
            "the zero step: once a frame"
        );
        assert_eq!(
            beside.agents[1].seen().runs,
            6 * frame,
            "the 1/120-s step: two a tick"
        );
    }
}

/// **With no fixed agent the published interpolation factor is the
/// gameplay remainder.** A half-tick frame publishes 0.5, the next half
/// tick completes the tick and publishes 0 — the same factor as with a fixed
/// agent, no longer the 0 the single clock published when nothing stepped.
/// Coverage: pins the new behaviour.
#[test]
fn with_no_fixed_agent_the_interpolation_factor_is_the_gameplay_remainder() {
    let mut rig = Rig::new(&[(AgentId::Script, None)], 1.0);
    rig.frame_of(TICK); // the first frame measured from here on
    let half = rig.frame_of(0.5 * TICK);
    assert!(
        (half.interpolation_alpha - 0.5).abs() < 0.05,
        "half a tick carried: {}",
        half.interpolation_alpha
    );
    let whole = rig.frame_of(0.5 * TICK);
    assert_eq!(whole.fixed_steps, 1);
    assert!(
        whole.interpolation_alpha < 0.05,
        "the tick taken: {}",
        whole.interpolation_alpha
    );
}

/// **Two fixed agents with different steps each integrate gameplay time.**
/// A 1/30-s agent beside a 1/120-s one: the scheduler runs both at the
/// smallest step, so the coarse agent — integrating its own 1/30 s each run
/// — sees four times the gameplay time. Under "the step is precision, never
/// time" each agent's time should stay within its step of the game's.
///
/// Not introduced by the two-clock change: the single clock ran every fixed
/// agent at the smallest step too.
#[test]
fn two_fixed_agents_with_different_steps_each_integrate_gameplay_time() {
    let mut rig = Rig::new(
        &[
            (AgentId::Physics, Some(gorna_step(120.0))),
            (AgentId::Audio, Some(gorna_step(30.0))),
        ],
        1.0,
    );
    for _ in 0..20 {
        rig.frame_of(0.05);
    }
    for (agent, hz) in rig.agents.iter().zip([120.0, 30.0]) {
        let seen = agent.seen();
        assert!(
            (seen.simulated - rig.gameplay).abs() < 1.0 / hz,
            "the 1/{hz}-s agent: {} s for {} s of gameplay ({} runs)",
            seen.simulated,
            rig.gameplay,
            seen.runs
        );
    }
}
