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

//! The frame start, apart from the frame: `begin_frame` lends the allocation
//! in force and swaps the staged layers in, before the host runs the frame's
//! `Boundary` systems; `run_frame` then runs on what it fixed, and begins a
//! frame itself only when the host did not.

use super::allocations::{self, allocation, envelope, lent_to, scheduler_over, Hand};
use super::layers::{self, scheduling, shared, Brush, Painter};
use crate::agent_registry::AgentRegistry;
use khora_core::agent::gorna::protocol::Epoch;
use khora_core::agent::gorna::StrategyId;
use khora_core::Runtime;
use khora_data::ecs::World;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// **`begin_frame` lends the allocation in force, before the frame runs.**
/// Epoch 1 is published; the frame start alone lends it to `ash` — what a
/// `Boundary` system, which runs between the two, sees as lent.
#[test]
fn begin_frame_lends_the_allocation_in_force() {
    let log = allocations::SharedLog::default();
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    let mut scheduler = scheduler_over(&registry);
    registry
        .lock()
        .expect("the registry lock")
        .register(Arc::new(Mutex::new(Hand::new("ash", &log))), 1.0);
    let runtime = Runtime::new();

    scheduler
        .budget_channel()
        .publish(allocation(1, &[("ash", 3_000)]));
    scheduler.begin_frame(&runtime);

    assert_eq!(
        lent_to(&log, "ash"),
        vec![(Epoch(1), envelope(3_000))],
        "lent at the frame start, before the frame runs"
    );
}

/// **A frame begun runs on what its start fixed.** Epoch 1 is lent by
/// `begin_frame`; epoch 2, published between the frame start and the frame —
/// while the `Boundary` systems run — is not lent by `run_frame`, which does
/// not begin the frame again: `ash` runs it on epoch 1. The next frame, not
/// begun by the host, begins itself and lends epoch 2.
#[test]
fn a_frame_begun_runs_on_the_allocation_its_start_lent() {
    let log = allocations::SharedLog::default();
    let registry = Arc::new(Mutex::new(AgentRegistry::new()));
    let mut scheduler = scheduler_over(&registry);
    registry
        .lock()
        .expect("the registry lock")
        .register(Arc::new(Mutex::new(Hand::new("ash", &log))), 1.0);
    let runtime = Arc::new(Runtime::new());
    let mut world = World::new();
    let channel = scheduler.budget_channel().clone();

    channel.publish(allocation(1, &[("ash", 3_000)]));
    scheduler.begin_frame(&runtime);
    channel.publish(allocation(2, &[("ash", 5_000)]));
    scheduler.run_frame(&mut world, Arc::clone(&runtime));

    assert_eq!(
        std::mem::take(&mut log.lock().expect("the log lock").ran),
        vec![(
            khora_core::agent::gorna::AgentId::new("ash"),
            Some(Epoch(1))
        )],
        "the frame runs on the epoch its start lent"
    );
    assert_eq!(lent_to(&log, "ash"), vec![(Epoch(1), envelope(3_000))]);

    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(
        lent_to(&log, "ash"),
        vec![(Epoch(1), envelope(3_000)), (Epoch(2), envelope(5_000))],
        "a frame the host did not begin begins itself"
    );
}

/// **`begin_frame` swaps a staged layer in.** The painter installed its
/// first layer; a second is staged at its place. The frame start alone
/// installs it, at the same place — before any `Boundary` system runs.
#[test]
fn begin_frame_swaps_a_staged_layer_in() {
    let log = layers::SharedLog::default();
    let painter = shared(Painter::new(
        "painter",
        Brush::new("painter", StrategyId::LowPower, &log),
        &log,
    ));
    let place = painter.lock().expect("the agent lock").place();
    let (mut scheduler, _rx) = scheduling(vec![Arc::clone(&painter)]);
    let runtime = Arc::new(Runtime::new());
    let mut world = World::new();
    scheduler.run_frame(&mut world, Arc::clone(&runtime));
    assert_eq!(
        place.lock().expect("the place lock").installed_row(),
        Some(StrategyId::LowPower)
    );

    place.lock().expect("the place lock").stage(
        Box::new(Brush::new("painter", StrategyId::HighPerformance, &log)),
        Duration::ZERO,
    );
    scheduler.begin_frame(&runtime);

    let place = place.lock().expect("the place lock");
    assert_eq!(place.installed_row(), Some(StrategyId::HighPerformance));
    assert_eq!(place.staged_row(), None);
}
