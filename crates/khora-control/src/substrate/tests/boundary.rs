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

//! **What spans frames lands at the boundary.** A `TickPhase::Boundary`
//! system takes what the runtime's reserve holds of its type through
//! `SystemContext::staged`; one serving a family that is off does not run,
//! and what the reserve holds for it waits until the family is back on.

use std::sync::{Arc, Mutex};

use crate::substrate::run_data_systems;
use khora_core::agent::gorna::AgentId;
use khora_core::agent::Activation;
use khora_core::event::Supersedes;
use khora_core::lane::{OutputDeck, Reserve, Staged, Staging};
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, Role, SystemContext, TickPhase, World};

/// The family the boundary system serves.
const FAMILY: AgentId = AgentId::new("boundary_test_family");

/// What a background run of the family staged.
#[derive(Debug, Clone, PartialEq)]
struct Recipe(u32);
impl Supersedes for Recipe {}
impl Staged for Recipe {}

/// Where the boundary system applies the recipes it takes: a runtime entry
/// only these tests' runtimes hold.
#[derive(Default)]
struct Applied(Mutex<Vec<Recipe>>);

fn apply_recipes(_: &mut World, runtime: &Runtime, _: &mut OutputDeck, ctx: SystemContext) {
    let Some(applied) = runtime.resources.get::<Applied>() else {
        return;
    };
    let taken = ctx.staged::<Recipe>(runtime);
    applied
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .extend(taken);
}

inventory::submit! {
    DataSystemRegistration {
        name: "boundary_test_apply_recipes",
        phase: TickPhase::Boundary,
        run: apply_recipes,
        order_hint: 0,
        runs_after: &[],
        observes: &[],
        role: Role::Serves(FAMILY),
    }
}

/// A runtime holding `reserve`, where the recipes are applied.
fn runtime_holding(reserve: &Reserve) -> Runtime {
    let mut runtime = Runtime::new();
    runtime.resources.insert(reserve.clone());
    runtime.resources.insert(Applied::default());
    runtime
}

/// A reserve holding what one run staged: `recipes`.
fn reserve_holding(recipes: &[u32]) -> Reserve {
    let reserve = Reserve::new();
    let mut staging = Staging::new();
    staging.stage(recipes.iter().map(|n| Recipe(*n)).collect());
    reserve.receive(staging);
    reserve
}

fn applied(runtime: &Runtime) -> Vec<Recipe> {
    runtime
        .resources
        .get::<Applied>()
        .expect("the applied recipes")
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

fn run_boundary(world: &mut World, runtime: &Runtime) {
    run_data_systems(world, runtime, &mut OutputDeck::new(), TickPhase::Boundary);
}

/// **A boundary system takes what the reserve holds**: the recipes a run
/// staged are applied at the boundary, in order, and the reserve is left
/// empty of them.
#[test]
fn a_boundary_system_takes_what_the_reserve_holds() {
    let reserve = reserve_holding(&[1, 2]);
    let runtime = runtime_holding(&reserve);
    let mut world = World::new();

    run_boundary(&mut world, &runtime);

    assert_eq!(applied(&runtime), vec![Recipe(1), Recipe(2)]);
    assert!(reserve.is_empty::<Recipe>());
}

/// **A boundary system serving a family that is off leaves the reserve
/// waiting.** While the family is off the system does not run, and the
/// recipe waits in the reserve; once the family is on, the next boundary
/// applies it.
#[test]
fn a_boundary_system_serving_a_family_off_leaves_the_reserve_waiting() {
    let reserve = reserve_holding(&[7]);
    let mut runtime = runtime_holding(&reserve);
    let activation = Arc::new(Activation::new([FAMILY]));
    runtime.resources.insert(Arc::clone(&activation));
    let mut world = World::new();
    activation.deactivate(FAMILY).expect("a known family");
    activation.settle();

    run_boundary(&mut world, &runtime);
    assert!(applied(&runtime).is_empty(), "the family is off");
    assert_eq!(reserve.len::<Recipe>(), 1, "the recipe waits");

    activation.activate(FAMILY).expect("a known family");
    activation.settle();
    run_boundary(&mut world, &runtime);
    assert_eq!(applied(&runtime), vec![Recipe(7)]);
    assert!(reserve.is_empty::<Recipe>());
}
