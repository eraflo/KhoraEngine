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

//! **A layered family's fixed step is its place's.** The step a game sets
//! (the physics step) is written into the family's place; the scheduler
//! reads it from there every frame, so a step changed between two frames
//! sets the number of sub-steps of the very next one.

use super::layers::{scheduling, shared, taken, Brush, SharedLog};
use super::{offer_of, report_of};
use khora_core::agent::gorna::protocol::{Envelope, Epoch, Offer, OfferRequest, Report};
use khora_core::agent::gorna::{AgentId, StrategyId};
use khora_core::agent::{Agent, ExecutionPhase, ExecutionTiming, Place, Wake};
use khora_core::time::DEFAULT_FIXED_DELTA_SECONDS;
use khora_core::{EngineContext, Runtime};
use khora_data::ecs::World;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A fixed-step family's agent: its place, shared with the test, holds its
/// step; it installs its one layer on its first wake.
struct Stepped {
    place: Arc<Mutex<Place>>,
    first: Option<Brush>,
}

impl Agent for Stepped {
    fn id(&self) -> AgentId {
        AgentId::new("physics")
    }
    fn offer(&mut self, request: &OfferRequest) -> Offer {
        offer_of(self.id(), request, StrategyId::Balanced)
    }
    fn apply(
        &mut self,
        epoch: Epoch,
        _envelope: &Envelope,
        _pin: Option<StrategyId>,
        _allowed: Option<&[StrategyId]>,
    ) {
        self.place.lock().expect("the place lock").lent(epoch);
    }
    fn report(&self) -> Report {
        report_of(self.id(), StrategyId::Balanced)
    }
    fn place(&self) -> Arc<Mutex<Place>> {
        Arc::clone(&self.place)
    }
    fn wake(&mut self, _why: &[Wake], place: &mut Place, _context: &mut EngineContext<'_>) {
        if place.installed_row().is_none() {
            if let Some(layer) = self.first.take() {
                place.stage(Box::new(layer), Duration::ZERO);
            }
        }
    }
    fn execution_timing(&self) -> ExecutionTiming {
        ExecutionTiming {
            allowed_phases: vec![ExecutionPhase::TRANSFORM],
            default_phase: ExecutionPhase::TRANSFORM,
            ..ExecutionTiming::default()
        }
    }
}

/// A step lasting a `per_tick`-th of a gameplay tick
/// (`DEFAULT_FIXED_DELTA_SECONDS`).
fn step_of(per_tick: u32) -> Duration {
    Duration::from_secs_f32(DEFAULT_FIXED_DELTA_SECONDS / per_tick as f32)
}

/// **The fixed step is read from the place each frame.** A layered family
/// starts at one step a gameplay tick. Between two one-tick frames its
/// place's step is set to a quarter of a tick: the next frame runs its layer
/// four times. Set to half a tick: the frame after runs it twice. A step
/// the scheduler kept from when it first met the agent would run it once a
/// frame throughout.
#[test]
fn the_fixed_step_is_read_from_the_place_each_frame() {
    let log = SharedLog::default();
    let start = step_of(1);
    let place = Arc::new(Mutex::new(Place::new(AgentId::new("physics"))));
    place
        .lock()
        .expect("the place lock")
        .set_fixed_step(Some(start));
    let agent = shared(Stepped {
        place: Arc::clone(&place),
        first: Some(Brush::new("physics", StrategyId::Balanced, &log)),
    });
    let (mut scheduler, _rx) = scheduling(vec![agent]);
    let mut world = World::new();
    let runtime = Arc::new(Runtime::new());
    let tick = Duration::from_secs_f32(DEFAULT_FIXED_DELTA_SECONDS);
    // One frame lasting a tick of wall clock, a hair over so float rounding
    // never drops it.
    let mut one_tick = |scheduler: &mut crate::scheduler::ExecutionScheduler| {
        scheduler.last_frame_instant = Some(Instant::now() - tick - Duration::from_micros(20));
        scheduler.run_frame(&mut world, Arc::clone(&runtime));
        taken(&log).ran.len()
    };

    assert_eq!(
        one_tick(&mut scheduler),
        1,
        "the first frame runs one step a tick"
    );

    place
        .lock()
        .expect("the place lock")
        .set_fixed_step(Some(step_of(4)));
    assert_eq!(
        one_tick(&mut scheduler),
        4,
        "a quarter-tick step set at the place runs four sub-steps the next frame"
    );

    place
        .lock()
        .expect("the place lock")
        .set_fixed_step(Some(step_of(2)));
    assert_eq!(
        one_tick(&mut scheduler),
        2,
        "a half-tick step set at the place runs two sub-steps the next frame"
    );
}
