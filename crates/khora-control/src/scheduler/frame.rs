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

//! The per-frame entry points: the frame start, and the frame run on what it fixed.
//! See the book: concepts/the-frame.md, "The six-stage descent".

use super::known::MetAgent;
use super::layers::{swap_layers, wake_agents};
use super::{AgentSelection, ExecutionScheduler, COMPONENT_ACCESS_SAMPLE_PERIOD};
use crate::substrate;
use khora_core::agent::completion::AgentCompletionMap;
use khora_core::agent::completion::CompletionOutcome;
use khora_core::agent::gorna::AgentId;
use khora_core::agent::{Activation, Agent, ExecutionPhase};
use khora_core::lane::{HostViews, LaneBus, OutputDeck};
use khora_core::telemetry::TelemetryEvent;
use khora_core::Runtime;
use khora_data::ecs::World;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::Instant;

impl ExecutionScheduler {
    /// Starts the frame: lends the allocation in force and swaps each family's staged
    /// layer in.
    ///
    /// Also takes back the background runs that ended, what they staged going to the
    /// runtime's [`Reserve`](khora_core::lane::Reserve). Call it once the families on and
    /// off are settled, before the frame's `Boundary` systems; [`run_frame`](Self::run_frame)
    /// begins a frame the host did not. See the book:
    /// concepts/agents-and-lanes.md, "Lanes are layers".
    pub fn begin_frame(&mut self, runtime: &Runtime) {
        if let Some(next) = self.budget_channel.take() {
            self.taken.push(next);
        }
        let inactive = inactive_families(runtime);
        // The runs that ended since come back first: a layer staged while its
        // family's run was out swaps in now that it is home.
        for (report, ms) in self.background.finish(runtime) {
            self.publish_report(Some(report), ms);
        }
        // And the layers prepared off the frame, in time for this frame
        // start's swap; a failed one wakes its agent at the event pass.
        self.background.take_prepared();
        // Then what the host staged since: later than any run that ended.
        if let Some(reserve) = runtime.look_up::<khora_core::lane::Reserve>() {
            reserve.land_from_host();
        }
        // Lent before anything locks an agent: the whole frame runs on one epoch.
        self.lend_allocation(&inactive);
        let met = self.met_agents(&inactive);
        swap_layers(&met, runtime);
        self.begun = true;
    }

    /// Runs one frame: the Flows, the event pass, then each phase's layers in waves.
    ///
    /// Called once a frame by the engine loop; it begins the frame itself when the host
    /// did not call [`begin_frame`](Self::begin_frame). The Flows project once a frame; a
    /// fixed sub-step is a run of its own, with its own report. See the book:
    /// concepts/the-frame.md, "Fixed-timestep simulation and render interpolation".
    pub fn run_frame(&mut self, world: &mut World, runtime: Arc<Runtime>) {
        self.run_frame_with(world, runtime, |_| {});
    }

    /// Runs one frame as [`run_frame`](Self::run_frame) does, handing `before_agents` the
    /// frame's bus before any agent runs.
    ///
    /// The bus then holds the Flows' and the host's Views: the SDK prepares the renderer
    /// from the camera View there, whatever agents run.
    pub fn run_frame_with(
        &mut self,
        world: &mut World,
        runtime: Arc<Runtime>,
        before_agents: impl FnOnce(&LaneBus),
    ) {
        // The frame start, unless the host began the frame itself.
        if !std::mem::take(&mut self.begun) {
            self.begin_frame(&runtime);
            self.begun = false;
        }
        self.frame_start = Instant::now();
        if let Ok(mut found) = self.undeclared_deck_writes.lock() {
            found.clear();
        }

        // Read once: the state the frame boundary settled, which the Flows and
        // DataSystems read too, so the whole frame agrees.
        let inactive = inactive_families(&runtime);

        // The families, the frame start having lent and swapped.
        let met = self.met_agents(&inactive);

        let fixed_rounds = self.advance_clocks(&runtime, &inactive);

        // Kept by the scheduler: agents do not read it through the runtime.
        let completion_map = self.completion_map();

        // Publish how agents will be grouped into concurrent waves this
        // frame, so the DCC can budget each wave by its critical path.
        self.emit_wave_plan(&inactive);

        let mut bus = LaneBus::new();
        let mut deck = OutputDeck::new();

        substrate::run_flows(world, &mut bus, &runtime);
        // Then the Views no Flow produces: the ones the host application set.
        if let Some(host_views) = runtime.resources.get::<Arc<HostViews>>() {
            host_views.publish_into(&mut bus);
        }
        before_agents(&bus);

        // The event pass: every agent whose place gathered a wake is woken
        // for it, once the bus is built and before any layer runs. A quiet
        // agent is neither called nor locked.
        wake_agents(&met, &runtime, &bus);

        // Read-only from here on; the worker pool needs an owned `Arc<LaneBus>`
        // to make a concurrent wave's jobs `'static`.
        let bus = Arc::new(bus);

        // The families at the background with mail start a run off the
        // frame, on this frame's bus; nothing waits for it.
        self.background.start(&met, &runtime, &bus);
        // The layers the event pass asked to prepare start too, beside
        // the installed ones, which keep running until they land.
        self.background.prepare(&met, &runtime, &bus);

        self.run_phases(
            &runtime,
            &inactive,
            &completion_map,
            &bus,
            &mut deck,
            fixed_rounds,
        );

        // Kept for the engine to drain at the I/O boundary.
        self.last_deck = deck;

        self.sample_component_access(world);
    }

    /// Returns the frame's completion map over every registered agent.
    ///
    /// A fixed agent owed no sub-step this frame is marked completed, so an agent
    /// depending on it (a finer-stepped one between two of its steps) still runs.
    fn completion_map(&self) -> Arc<AgentCompletionMap> {
        let agent_ids = self
            .registry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .all_ids();
        let completion_map = Arc::new(AgentCompletionMap::new(&agent_ids));
        for (&id, &due) in &self.fixed_due {
            if due == 0 {
                completion_map.mark(id, CompletionOutcome::Completed);
            }
        }
        completion_map
    }

    /// Runs the frame's phases, each family's installed layer at its place.
    ///
    /// With `fixed_rounds`, the fixed agents first run their sub-steps round by round, and
    /// are then left out of the once-per-frame loop.
    fn run_phases(
        &mut self,
        runtime: &Arc<Runtime>,
        inactive: &BTreeSet<AgentId>,
        completion_map: &Arc<AgentCompletionMap>,
        bus: &Arc<LaneBus>,
        deck: &mut OutputDeck,
        fixed_rounds: Option<u32>,
    ) {
        // Each sub-step is a full run, so physics integrates N times while the
        // render below fires once.
        let phases: Vec<ExecutionPhase> = self.phase_order.clone();
        if let Some(rounds) = fixed_rounds {
            for round in 0..rounds {
                for &phase in &phases {
                    self.execute_agents_in_phase(
                        phase,
                        runtime,
                        inactive,
                        completion_map,
                        bus,
                        deck,
                        AgentSelection::FixedRound(round),
                    );
                }
            }
        }

        let selection = if fixed_rounds.is_some() {
            AgentSelection::ExcludeFixed
        } else {
            AgentSelection::All
        };

        for phase in phases {
            self.execute_agents_in_phase(
                phase,
                runtime,
                inactive,
                completion_map,
                bus,
                deck,
                selection,
            );
        }
    }

    /// Publishes per-component access snapshots for the DCC's layout advisor.
    ///
    /// Sampled every `COMPONENT_ACCESS_SAMPLE_PERIOD` frames, best-effort: dropped when the
    /// channel is full.
    fn sample_component_access(&mut self, world: &World) {
        self.frame_counter = self.frame_counter.wrapping_add(1);
        if let Some(tx) = &self.telemetry {
            if self
                .frame_counter
                .is_multiple_of(COMPONENT_ACCESS_SAMPLE_PERIOD)
            {
                for (type_name, size_bytes, query_count, rows_scanned) in
                    world.component_access_snapshot()
                {
                    let _ = tx.try_send(TelemetryEvent::ComponentAccess {
                        type_name,
                        size_bytes,
                        query_count,
                        rows_scanned,
                    });
                }
            }
        }
    }

    /// Returns every agent of a family on, with its lock and what the scheduler knows of it.
    fn met_agents(&self, inactive: &BTreeSet<AgentId>) -> Vec<MetAgent> {
        let agents: Vec<Arc<Mutex<dyn Agent>>> = {
            let registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
            registry.iter_active(inactive).cloned().collect()
        };
        agents
            .into_iter()
            .filter_map(|agent| {
                let known = self.known.of(&agent)?;
                Some((agent, known))
            })
            .collect()
    }
}

/// The agents whose family is off, as the runtime's [`Activation`] holds
/// them; none without one.
fn inactive_families(runtime: &Runtime) -> BTreeSet<AgentId> {
    runtime
        .resources
        .get::<Arc<Activation>>()
        .map(|activation| activation.inactive())
        .unwrap_or_default()
}
