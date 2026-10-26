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

//! What boot says about a family registered in a phase the scheduler never
//! runs.

use std::any::TypeId;

use khora_core::agent::registration::{check_registrations_in, AgentRegistration, BootError};
use khora_core::agent::{AgentImportance, ExecutionPhase};
use khora_core::gpu::DeviceCaps;
use khora_core::lane::erased::{AnyLane, Erased};
use khora_core::lane::family::LaneFamily;
use khora_core::Runtime;

use crate::fixtures::{MixFamily, MixLane, MixSettings};

fn mix_lane() -> Box<dyn AnyLane> {
    Box::new(Erased::new(MixLane))
}

/// The mix family in `phase`.
fn mix_in(phase: ExecutionPhase) -> AgentRegistration {
    AgentRegistration {
        family: TypeId::of::<MixFamily>(),
        family_name: MixFamily::NAME,
        kind: MixFamily::KIND,
        agent: MixFamily::AGENT,
        semantics: MixFamily::SEMANTICS,
        phase,
        importance: AgentImportance::Important,
        after: &[],
        modes: &[],
        fixed_step: None,
        lanes: &[mix_lane],
    }
}

fn runtime() -> Runtime {
    let mut runtime = Runtime::new();
    runtime.resources.insert(MixSettings { gain: 1.0 });
    runtime
}

/// **A family in a phase the game removed never runs, and boot says so.**
/// Its agent would be registered and offered budgets, yet never once execute;
/// boot refuses it, naming the family and the phase.
#[test]
fn boot_refuses_a_family_whose_phase_never_runs() {
    let mix = mix_in(ExecutionPhase::OBSERVE);
    let without_observe: Vec<ExecutionPhase> = ExecutionPhase::DEFAULT_ORDER
        .iter()
        .copied()
        .filter(|phase| *phase != ExecutionPhase::OBSERVE)
        .collect();

    let checked = check_registrations_in(
        &[&mix],
        &runtime(),
        &DeviceCaps::default(),
        &without_observe,
    );

    assert_eq!(
        checked,
        Err(BootError::PhaseNotRun {
            family: "mix",
            phase: ExecutionPhase::OBSERVE,
        })
    );
}

/// A game's own phase in the order runs like a built-in one: a family in it
/// boots.
#[test]
fn a_family_in_a_games_own_phase_boots() {
    let late = ExecutionPhase::custom(6);
    let mix = mix_in(late);
    let mut order = ExecutionPhase::DEFAULT_ORDER.to_vec();
    order.insert(5, late);

    let checked = check_registrations_in(&[&mix], &runtime(), &DeviceCaps::default(), &order);

    assert_eq!(checked, Ok(()));
}
