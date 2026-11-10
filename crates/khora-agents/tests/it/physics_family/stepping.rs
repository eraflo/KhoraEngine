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

//! What one run of the physics agent advances: one step of the size the
//! scheduler cuts the gameplay time by — never another.

use std::sync::Arc;

use khora_core::lane::OutputDeck;
use khora_data::flow::PhysicsStepResult;

use super::fixtures::{
    physics_agent, physics_runtime, place_step, recorded, run_once, ScriptedProvider,
};

/// **A run is one step of the agent's own size.** The scheduler gives the
/// agent one run per whole step its place holds; a lane
/// stepping the provider by anything else would run physics faster or slower
/// than gameplay. The step the deck reports is the step the provider took.
#[test]
fn the_physics_agent_steps_the_provider_by_its_fixed_step() {
    let (provider, steps) = ScriptedProvider::new(Vec::new(), 0);
    let runtime = Arc::new(physics_runtime(&provider.shared()));
    let mut agent = physics_agent();
    let step = place_step(&agent)
        .expect("physics runs at a fixed step")
        .as_secs_f32();
    let mut deck = OutputDeck::new();

    run_once(&mut agent, &runtime, &mut deck);

    assert_eq!(recorded(&steps), vec![step], "one step of the agent's size");
    let reported = deck
        .get::<PhysicsStepResult>()
        .expect("the agent committed the step result");
    assert_eq!(reported.dt, step, "the deck reports the step taken");
}

/// The sub-steps of one frame are each one step: three runs into one deck
/// step the provider three times, by the same size.
#[test]
fn each_sub_step_of_a_frame_is_one_physics_step() {
    let (provider, steps) = ScriptedProvider::new(Vec::new(), 0);
    let runtime = Arc::new(physics_runtime(&provider.shared()));
    let mut agent = physics_agent();
    let step = place_step(&agent)
        .expect("physics runs at a fixed step")
        .as_secs_f32();
    let mut deck = OutputDeck::new();

    for _ in 0..3 {
        run_once(&mut agent, &runtime, &mut deck);
    }

    assert_eq!(recorded(&steps), vec![step; 3]);
    assert!(
        deck.contains::<PhysicsStepResult>(),
        "the frame's step result is on the deck"
    );
}

/// **A step written at the place is the step the family runs at.** The place
/// is where the scheduler reads the family's step each frame; a step written
/// there (`Place::set_fixed_step`) between two frames is the one the next
/// frame's runs take — the agent's next wake, which hands the place its
/// terms again, does not put back the step the agent was built with.
#[test]
fn a_step_written_at_the_place_holds_across_the_agents_next_wake() {
    let (provider, steps) = ScriptedProvider::new(Vec::new(), 0);
    let runtime = Arc::new(physics_runtime(&provider.shared()));
    let mut agent = physics_agent();
    let a_30th = std::time::Duration::from_nanos(33_333_333);
    {
        use khora_core::agent::Agent;
        agent
            .place()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .set_fixed_step(Some(a_30th));
    }
    let mut deck = OutputDeck::new();

    // The first frame wakes the agent (its first-frame wake), then runs.
    run_once(&mut agent, &runtime, &mut deck);

    assert_eq!(
        place_step(&agent),
        Some(a_30th),
        "the place still holds the step written there"
    );
    assert_eq!(
        recorded(&steps),
        vec![a_30th.as_secs_f32()],
        "the run stepped the provider by the place's step"
    );
}
