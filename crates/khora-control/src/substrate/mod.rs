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

//! Substrate Pass — orchestrates the Data layer's self-maintenance work.
//!
//! Per CLAD's enriched doctrine, the Substrate Pass runs *around* the
//! command path (`Control → Agent → Lane → Data`), not inside it. It is
//! invoked by the Scheduler at well-defined points in the tick:
//!
//! - [`khora_data::ecs::TickPhase::PreSimulation`] before any agent simulates,
//! - [`khora_data::ecs::TickPhase::PostSimulation`] before extraction,
//! - [`khora_data::ecs::TickPhase::PreExtract`] right before `Flow`s project,
//! - [`khora_data::ecs::TickPhase::Maintenance`] at the end of the tick.
//!
//! The pass discovers `DataSystemRegistration` entries via [`inventory`],
//! orders them by `runs_after` (topological sort) with `order_hint` as a
//! tie-breaker, and invokes them sequentially.

pub mod flow_runner;

pub use flow_runner::run_flows;

use khora_core::graph::topological_sort;
use khora_core::Runtime;
use khora_data::ecs::{DataSystemRegistration, TickPhase, World};
use std::collections::HashMap;

/// Runs every [`DataSystemRegistration`] declared for the given phase, in a
/// stable order: topological by `runs_after`, then by `order_hint`, then by
/// `name` for full determinism.
///
/// On a cycle in `runs_after` the function logs an error and falls back to
/// the `(order_hint, name)` ordering — execution still happens, just not in
/// DAG order. Validating cycles at registration time is a future improvement.
pub fn run_data_systems(world: &mut World, runtime: &Runtime, phase: TickPhase) {
    let mut systems: Vec<&'static DataSystemRegistration> =
        inventory::iter::<DataSystemRegistration>
            .into_iter()
            .filter(|s| s.phase == phase)
            .collect();

    if systems.is_empty() {
        return;
    }

    sort_systems(&mut systems);

    for sys in systems {
        (sys.run)(world, runtime);
    }
}

/// Stable in-place sort over a phase's systems.
///
/// Topological by `runs_after` (Kahn's algorithm via
/// [`topological_sort`]), with `(order_hint, name)` lifted as the
/// tie-breaker among ready-equal nodes. On a cycle, falls back to
/// `(order_hint, name)` only and logs an error.
fn sort_systems(systems: &mut Vec<&'static DataSystemRegistration>) {
    // Lift `(order_hint, name)` as the deterministic baseline. Kahn's
    // ready queue is a FIFO, so this baseline ordering carries through to
    // the topological output as the tie-breaker.
    systems.sort_by(|a, b| {
        a.order_hint
            .cmp(&b.order_hint)
            .then_with(|| a.name.cmp(b.name))
    });

    let by_name: HashMap<&'static str, &'static DataSystemRegistration> =
        systems.iter().map(|s| (s.name, *s)).collect();

    let nodes: Vec<&'static str> = systems.iter().map(|s| s.name).collect();
    let edges: Vec<(&'static str, &'static str)> = systems
        .iter()
        .flat_map(|s| {
            s.runs_after
                .iter()
                .filter(|dep| by_name.contains_key(*dep))
                .map(move |dep| (*dep, s.name))
        })
        .collect();

    match topological_sort(nodes, edges) {
        Ok(order) => {
            *systems = order
                .into_iter()
                .filter_map(|name| by_name.get(name).copied())
                .collect();
        }
        Err(_) => {
            log::error!(
                "substrate: cycle detected in DataSystem `runs_after` for phase {:?} — \
                 falling back to (order_hint, name) order",
                systems.first().map(|s| s.phase),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ORDER_LOG: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

    fn record(name: &'static str) {
        ORDER_LOG.lock().unwrap().push(name);
    }

    fn sys_a(_: &mut World, _: &Runtime) {
        record("a");
    }
    fn sys_b(_: &mut World, _: &Runtime) {
        record("b");
    }
    fn sys_c(_: &mut World, _: &Runtime) {
        record("c");
    }

    inventory::submit! {
        DataSystemRegistration {
            name: "test_b",
            phase: TickPhase::PreSimulation,
            run: sys_b,
            order_hint: 10,
            runs_after: &[],
        }
    }
    inventory::submit! {
        DataSystemRegistration {
            name: "test_a",
            phase: TickPhase::PreSimulation,
            run: sys_a,
            order_hint: 0,
            runs_after: &[],
        }
    }
    inventory::submit! {
        DataSystemRegistration {
            name: "test_c",
            phase: TickPhase::PreSimulation,
            run: sys_c,
            order_hint: -5,
            runs_after: &["test_a"], // Forces c after a despite lower hint.
        }
    }

    #[test]
    fn topo_order_respects_runs_after() {
        ORDER_LOG.lock().unwrap().clear();

        let mut world = World::new();
        let runtime = Runtime::new();
        run_data_systems(&mut world, &runtime, TickPhase::PreSimulation);

        let log = ORDER_LOG.lock().unwrap();
        // Hard guarantee: c must come after a (declared `runs_after: ["test_a"]`).
        let pos_a = log.iter().position(|n| *n == "a").unwrap();
        let pos_c = log.iter().position(|n| *n == "c").unwrap();
        assert!(
            pos_a < pos_c,
            "topo order violated: a should run before c (got {:?})",
            *log
        );

        // Soft guarantee: among nodes ready at the same time, lower order_hint
        // wins as a tie-breaker. a (hint=0) and b (hint=10) are both ready
        // initially; a must come before b.
        let pos_b = log.iter().position(|n| *n == "b").unwrap();
        assert!(
            pos_a < pos_b,
            "tie-breaker violated: a should run before b (got {:?})",
            *log
        );
    }
}
