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

//! **The physics step is precision, never time.** The gameplay tick
//! ([`DEFAULT_FIXED_DELTA_SECONDS`]) is the one clock game code sees: the
//! accumulator, the time scale, the [`MAX_SIM_STEPS`]-tick catch-up cap and
//! the published `Time` all run on it. The step a fixed agent declares (the
//! physics GORNA strategy: 1/30, 1/60 or 1/120 s) only subdivides the time
//! those ticks advanced: whatever the step, and however often it changes,
//! physics time stays within one step of gameplay time, and nothing a game
//! reads depends on it.
//!
//! The `Falling` agent below declares its step through a shared cell — so a
//! test can switch it between frames, as GORNA's `apply_budget` does — and on
//! each run integrates a falling body with that step (semi-implicit Euler),
//! adding the step to the simulated time it has seen.
//!
//! The frames are given a wall-clock length by placing the previous frame's
//! instant that far in the past; a few microseconds are added so the measured
//! delta is never a hair below the length it is meant to cover. Frame lengths
//! are chosen in quarter ticks so that the few microseconds a frame gains
//! never carry the gameplay accumulator across a tick boundary.

use crate::agent_registry::AgentRegistry;
use crate::dcc_context::Context;
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

/// Gravity, m/s².
const G: f64 = 9.81;

/// The gameplay tick, in seconds.
const TICK: f64 = DEFAULT_FIXED_DELTA_SECONDS as f64;

/// The physics steps GORNA chooses between, in hertz.
const GORNA_RATES: [f64; 3] = [30.0, 60.0, 120.0];

/// The real frame clamp the scheduler applies, in ticks.
const MAX_FRAME_TICKS: f64 = 15.0;

fn step_of(hz: f64) -> Duration {
    Duration::from_secs_f64(1.0 / hz)
}

/// A body falling from rest, and the simulated time it was integrated over.
#[derive(Debug, Default, Clone, Copy)]
struct Body {
    velocity: f64,
    fallen: f64,
    simulated: f64,
    runs: usize,
}

/// Declares the step in `step` as its fixed timestep and, on each run,
/// integrates `body` by that step.
struct Falling {
    step: Arc<Mutex<Duration>>,
    body: Arc<Mutex<Body>>,
}

impl Falling {
    fn step(&self) -> Duration {
        *self.step.lock().expect("the step lock")
    }
}

impl Agent for Falling {
    fn id(&self) -> AgentId {
        AgentId::Physics
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
            agent_id: AgentId::Physics,
            current_strategy: StrategyId::Balanced,
            health_score: 1.0,
            is_stalled: false,
            message: String::new(),
        }
    }
    fn execute(&mut self, _ctx: &mut EngineContext<'_>) {
        let p = self.step().as_secs_f64();
        let mut body = self.body.lock().expect("the body lock");
        body.velocity += G * p;
        body.fallen += body.velocity * p;
        body.simulated += p;
        body.runs += 1;
    }
    fn execution_timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            allowed_phases: vec![ExecutionPhase::TRANSFORM],
            default_phase: ExecutionPhase::TRANSFORM,
            fixed_timestep: Some(self.step()),
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

/// A scheduler running one `Falling` agent at `step`, with the world, the
/// runtime and its clock at `scale`.
struct Rig {
    scheduler: ExecutionScheduler,
    step: Arc<Mutex<Duration>>,
    body: Arc<Mutex<Body>>,
    world: World,
    runtime: Arc<Runtime>,
    time: SharedTime,
}

impl Rig {
    fn new(step: Duration, scale: f32) -> Self {
        let step = Arc::new(Mutex::new(step));
        let body = Arc::new(Mutex::new(Body::default()));
        let registry = Arc::new(Mutex::new(AgentRegistry::new()));
        registry.lock().expect("the registry lock").register(
            Arc::new(Mutex::new(Falling {
                step: Arc::clone(&step),
                body: Arc::clone(&body),
            })),
            1.0,
        );
        let scheduler = ExecutionScheduler::new(
            registry,
            Arc::new(RwLock::new(Context::default())),
            &[AgentId::Physics],
        );
        let mut clock = Time::default();
        clock.set_scale(scale);
        let time: SharedTime = Arc::new(RwLock::new(clock));
        let mut runtime = Runtime::new();
        runtime.resources.insert(Arc::clone(&time));
        Self {
            scheduler,
            step,
            body,
            world: World::new(),
            runtime: Arc::new(runtime),
            time,
        }
    }

    /// Runs one frame lasting `seconds` of wall clock; the frame's `Time`.
    fn frame_of(&mut self, seconds: f64) -> Time {
        let length = Duration::from_secs_f64(seconds) + Duration::from_micros(20);
        self.scheduler.last_frame_instant = Some(Instant::now() - length);
        self.run()
    }

    /// Runs one frame without touching the previous frame's instant.
    fn run(&mut self) -> Time {
        self.scheduler
            .run_frame(&mut self.world, Arc::clone(&self.runtime));
        *self.time.read().expect("the clock lock")
    }

    fn switch_to(&self, step: Duration) {
        *self.step.lock().expect("the step lock") = step;
    }

    fn body(&self) -> Body {
        *self.body.lock().expect("the body lock")
    }
}

/// The gameplay time, in seconds, the contract advances over `frames` (in
/// ticks): the real delta clamped to a quarter of a second, accumulated,
/// consumed in whole ticks capped at [`MAX_SIM_STEPS`] a frame — the excess
/// whole ticks dropped — and the remainder carried. Counted in quarter ticks
/// so it is exact.
fn gameplay_seconds(frames_in_ticks: &[f64]) -> Vec<f64> {
    let mut accumulator = 0u64;
    let mut ticks = 0u64;
    frames_in_ticks
        .iter()
        .map(|&frame| {
            let quarters = (frame.min(MAX_FRAME_TICKS) * 4.0).round() as u64;
            accumulator += quarters;
            let whole = accumulator / 4;
            if whole > u64::from(MAX_SIM_STEPS) {
                ticks += u64::from(MAX_SIM_STEPS);
                accumulator %= 4;
            } else {
                ticks += whole;
                accumulator -= whole * 4;
            }
            ticks as f64 * TICK
        })
        .collect()
}

/// The render interpolation factor the contract publishes after each of
/// `frames` (in ticks): the carried gameplay remainder over the tick.
fn gameplay_alphas(frames_in_ticks: &[f64]) -> Vec<f64> {
    let mut accumulator = 0u64;
    frames_in_ticks
        .iter()
        .map(|&frame| {
            accumulator += (frame.min(MAX_FRAME_TICKS) * 4.0).round() as u64;
            let whole = accumulator / 4;
            if whole > u64::from(MAX_SIM_STEPS) {
                accumulator %= 4;
            } else {
                accumulator -= whole * 4;
            }
            accumulator as f64 / 4.0
        })
        .collect()
}

/// **A second of 1/20-s frames is a second of falling under every physics
/// step.** Twenty frames of three ticks each: the body integrated at 1/30,
/// 1/60 or 1/120 s a step has seen one second of simulated time, to within a
/// step, and fallen the analytic `g·t²/2 = 4.905 m` to within the
/// integration error — semi-implicit Euler over `t = N·p` falls
/// `g·t·(t + p)/2`, and with `t` in `(1 − p, 1]` that is at most
/// `g·p/2 ≈ 0.16 m` off at `p = 1/30`; 0.2 m holds it.
///
/// Counting the catch-up cap in physics steps instead of ticks, five steps
/// of 1/120 s a frame would cover 0.83 s of the second.
#[test]
fn a_second_of_twentieth_of_a_second_frames_falls_the_same_under_every_physics_step() {
    for hz in GORNA_RATES {
        let p = 1.0 / hz;
        let mut rig = Rig::new(step_of(hz), 1.0);

        for _ in 0..20 {
            rig.frame_of(0.05);
        }

        let body = rig.body();
        assert!(
            (body.simulated - 1.0).abs() < p,
            "1/{hz} s a step: {} s of physics for a second of frames ({} runs)",
            body.simulated,
            body.runs
        );
        assert!(
            (body.fallen - 0.5 * G).abs() < 0.2,
            "1/{hz} s a step: fell {} m in a second, not {} m",
            body.fallen,
            0.5 * G
        );
    }
}

/// **Every physics step sees the gameplay time, at 60 FPS and under an
/// irregular frame mix.** A second of 1/60-s frames, then frames from half a
/// tick to a clamped 0.3 s — some over the catch-up cap — run under 1/30,
/// 1/60 and 1/120 s: after every frame, the physics time is within one step
/// of the gameplay time the ticks advanced, so all three agree with each
/// other whatever the frame rate.
#[test]
fn every_physics_step_sees_the_gameplay_time_at_sixty_fps_and_under_irregular_frames() {
    let sixty_fps = vec![1.0; 60];
    let irregular = vec![
        1.0, 2.5, 0.5, 4.0, 1.5, 6.0, 0.5, 3.0, 15.0, 2.0, 0.5, 18.0, 1.5, 3.5, 0.5, 4.5, 2.0,
    ];
    for (name, frames) in [("60 FPS", sixty_fps), ("irregular", irregular)] {
        let expected = gameplay_seconds(&frames);
        for hz in GORNA_RATES {
            let p = 1.0 / hz;
            let mut rig = Rig::new(step_of(hz), 1.0);
            for (i, (&ticks, &gameplay)) in frames.iter().zip(&expected).enumerate() {
                rig.frame_of(ticks * TICK);
                let physics = rig.body().simulated;
                assert!(
                    (physics - gameplay).abs() < p,
                    "{name}, 1/{hz} s a step, frame {i} ({ticks} ticks): \
                     {physics} s of physics for {gameplay} s of gameplay"
                );
            }
        }
    }
}

/// **The published `Time` is the gameplay tick under every physics step**:
/// `fixed_delta_seconds` is [`DEFAULT_FIXED_DELTA_SECONDS`] and
/// `fixed_steps` the ticks the frame took — a 1/30-s frame is two ticks
/// whether physics steps at 1/30 s or 1/120 s, a quarter of a second the
/// cap's five, a 1/20-s frame three, and two 1/120-s frames none then one.
#[test]
fn the_published_time_counts_gameplay_ticks_under_every_physics_step() {
    let frames = [1.0 / 30.0, 0.25, 1.0 / 20.0, 1.0 / 120.0, 1.0 / 120.0];
    let ticks = [2, MAX_SIM_STEPS, 3, 0, 1];
    for hz in GORNA_RATES {
        let mut rig = Rig::new(step_of(hz), 1.0);
        for (&seconds, &expected) in frames.iter().zip(&ticks) {
            let time = rig.frame_of(seconds);
            assert_eq!(
                time.fixed_delta_seconds, DEFAULT_FIXED_DELTA_SECONDS,
                "1/{hz} s a step, a {seconds}-s frame: the published step is the tick"
            );
            assert_eq!(
                time.fixed_steps, expected,
                "1/{hz} s a step, a {seconds}-s frame: the ticks it took"
            );
        }
    }
}

/// **Switching the physics step between frames keeps physics on gameplay
/// time.** Sixty 1/20-s frames with the step switched before nearly every
/// one (1/60 → 1/120 → 1/30 → …, as GORNA re-budgets): after every frame
/// the physics time is within one step — the coarsest, 1/30 s — of the
/// three ticks a frame the gameplay clock advanced.
#[test]
fn switching_the_physics_step_between_frames_keeps_physics_on_gameplay_time() {
    let schedule = [60.0, 120.0, 30.0, 120.0, 60.0, 30.0, 30.0, 120.0, 60.0];
    let coarsest = 1.0 / 30.0;
    let mut rig = Rig::new(step_of(schedule[0]), 1.0);

    for frame in 0..60usize {
        let hz = schedule[frame % schedule.len()];
        rig.switch_to(step_of(hz));
        rig.frame_of(0.05);

        let gameplay = (frame + 1) as f64 * 3.0 * TICK;
        let physics = rig.body().simulated;
        assert!(
            (physics - gameplay).abs() < coarsest,
            "frame {frame} at 1/{hz} s a step: {physics} s of physics for {gameplay} s of gameplay"
        );
    }
    assert!(
        rig.body().runs > 60,
        "set-up: the body was sub-stepped — {} runs",
        rig.body().runs
    );
}

/// **A quarter-second frame is five ticks of physics under every step**:
/// the catch-up cap is five gameplay ticks (5/60 s), not five physics steps
/// — two steps of 1/30 s (2.5 owed, half carried), five of 1/60 s, ten of
/// 1/120 s.
#[test]
fn a_quarter_second_frame_advances_five_ticks_of_physics_under_every_step() {
    for (hz, sub_steps) in [(30.0, 2), (60.0, 5), (120.0, 10)] {
        let p = 1.0 / hz;
        let mut rig = Rig::new(step_of(hz), 1.0);

        rig.frame_of(0.25);

        let body = rig.body();
        assert_eq!(body.runs, sub_steps, "1/{hz} s a step: sub-steps run");
        let capped = f64::from(MAX_SIM_STEPS) * TICK;
        assert!(
            (body.simulated - capped).abs() < p,
            "1/{hz} s a step: {} s of physics for the cap's {capped} s",
            body.simulated
        );
    }
}

/// **The render interpolation factor does not depend on the physics step**:
/// over the same frames — quarter ticks to a capped 6.5 ticks — the
/// published `interpolation_alpha` is the carried gameplay remainder over the
/// tick, frame by frame, under 1/30, 1/60 and 1/120 s.
#[test]
fn the_interpolation_alpha_is_the_same_under_every_physics_step() {
    let frames = [0.25, 0.5, 1.0, 1.75, 0.25, 2.5, 0.75, 3.0, 1.25, 6.5, 0.5];
    let expected = gameplay_alphas(&frames);
    for hz in GORNA_RATES {
        let mut rig = Rig::new(step_of(hz), 1.0);
        for (i, (&ticks, &alpha)) in frames.iter().zip(&expected).enumerate() {
            let published = f64::from(rig.frame_of(ticks * TICK).interpolation_alpha);
            assert!(
                (published - alpha).abs() < 0.05,
                "1/{hz} s a step, frame {i} ({ticks} ticks): alpha {published}, not {alpha}"
            );
        }
    }
}

/// **Paused, no physics step runs under any step**: at scale 0, eight
/// 1/30-s frames run the fixed agent not once and publish no tick; resumed,
/// a 1/30-s frame advances the physics by that frame's two ticks only —
/// nothing was owed from the pause.
#[test]
fn paused_no_physics_step_runs_under_any_step() {
    for hz in GORNA_RATES {
        let p = 1.0 / hz;
        let mut rig = Rig::new(step_of(hz), 0.0);

        for _ in 0..8 {
            let time = rig.frame_of(1.0 / 30.0);
            assert_eq!(time.fixed_steps, 0, "1/{hz} s a step, paused: no tick");
        }
        assert_eq!(rig.body().runs, 0, "1/{hz} s a step, paused: no sub-step");

        rig.time.write().expect("the clock lock").set_scale(1.0);
        rig.frame_of(1.0 / 30.0);
        let physics = rig.body().simulated;
        assert!(
            (physics - 2.0 * TICK).abs() < p,
            "1/{hz} s a step, resumed: {physics} s of physics for a 1/30-s frame"
        );
    }
}

/// **The first frame advances one gameplay tick under every step**: with no
/// previous frame to measure, the scheduler advances one tick — published as
/// one step of [`DEFAULT_FIXED_DELTA_SECONDS`] — which is no step of 1/30 s
/// (the tick is carried), one of 1/60 s and two of 1/120 s.
#[test]
fn the_first_frame_advances_one_gameplay_tick_under_every_step() {
    for (hz, sub_steps) in [(30.0, 0), (60.0, 1), (120.0, 2)] {
        let mut rig = Rig::new(step_of(hz), 1.0);

        let time = rig.run();

        assert_eq!(time.fixed_steps, 1, "1/{hz} s a step: one tick");
        assert_eq!(time.fixed_delta_seconds, DEFAULT_FIXED_DELTA_SECONDS);
        assert_eq!(rig.body().runs, sub_steps, "1/{hz} s a step: sub-steps run");
    }
}
