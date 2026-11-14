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

//! A project's systems and behaviors played over a world:
//! one session wired the way the engine wires one — declarations staged in
//! the reserve and programs waiting for the intake, what each program reads
//! announced to the catalog, the engine's `Activation` when the test switches
//! the scripts, the `Boundary` and `PreExtract` data systems, the flow, the script lane run with a chosen
//! fuel through an `InputSource` as its family's agent lends one, then
//! `Maintenance` applying and writing back.

use std::sync::{Arc, Mutex};

use khora_control::substrate::run_data_systems;
use khora_core::agent::gorna::StrategyId;
use khora_core::agent::Activation;
use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;
use khora_core::event::{discard_mail_of_inactive, Channel, WhenFull};
use khora_core::lane::cost::Cost;
use khora_core::lane::{OutputDeck, Reserve};
use khora_core::projection::Interest;
use khora_core::script::{
    engine_event_channel, CommandBuffer, ComponentName, ScriptCatalog, ScriptEvent, ScriptReads,
    WorldCommand,
};
use khora_core::Runtime;
use khora_data::ecs::{TickPhase, World};
use khora_data::flow::{Flow, FlowContext, ScriptFlow, ScriptView};
use khora_io::script::declarations::DeclaredComponents;
use khora_lanes::script::{take_in, ScriptRuntime};
use khora_script::reload::ScriptReload;
use khora_script::vm::Program;

use super::behavior_session::{compile, declared_in, key, stage_declarations, Project, PLENTY};
use super::fixtures::{no_prefabs, quote_lane, run_lane, LaneFrame};

/// One running session over a world, its scripts on or off as the test
/// switches them.
pub(crate) struct SystemSession {
    pub(crate) world: World,
    pub(crate) scripts: ScriptRuntime,
    /// The fuel each frame's lane runs with.
    pub(crate) fuel: u64,
    /// The last playing frame's view, as the lane read it.
    last_view: Option<ScriptView>,
    flow: ScriptFlow,
    /// Whether the flow ran before, as the substrate keeps it: its first run
    /// is a resumption.
    projected: bool,
    runtime: Arc<Runtime>,
    /// The engine's switch, when the test switches the scripts; without
    /// one, every family is on.
    activation: Option<Arc<Activation>>,
    reloads: Channel<ScriptReload>,
    events: Channel<ScriptEvent>,
    reads: Arc<Interest<ScriptReads>>,
    /// Whether the last frame played, and took in what the engine handed
    /// over: a playing frame after one that did not starts play over.
    took_in: bool,
}

impl SystemSession {
    /// A session over `world` playing `project`.
    pub(crate) fn new(world: World, project: Project) -> Self {
        Self::over(world, project, None)
    }

    /// A session over `world` playing `project`, whose scripts the test
    /// switches with [`set_scripts`](Self::set_scripts).
    pub(crate) fn switchable(world: World, project: Project) -> Self {
        let activation = Arc::new(Activation::new([khora_core::script::SCRIPT]));
        Self::over(world, project, Some(activation))
    }

    /// A session over `world` whose scripts are off — an editor editing a
    /// scene — until [`set_scripts`](Self::set_scripts) switches them on.
    pub(crate) fn with_scripts_off(world: World, project: Project) -> Self {
        let activation = Arc::new(Activation::new([khora_core::script::SCRIPT]));
        activation
            .deactivate(khora_core::script::SCRIPT)
            .expect("a known family");
        Self::over(world, project, Some(activation))
    }

    /// A session over `world`, with `project`'s modules handed over: every
    /// module's declarations registered at once and its program waiting for
    /// the lane, what it reads announced.
    fn over(world: World, project: Project, activation: Option<Arc<Activation>>) -> Self {
        let declarations = Reserve::new();
        let reloads = Channel::bounded(1024, WhenFull::DropOldest);
        let events = engine_event_channel();
        let catalog = Arc::new(ScriptCatalog::default());
        let reads = Arc::new(Interest::<ScriptReads>::new());

        let mut runtime = Runtime::default();
        runtime.resources.insert(declarations.clone());
        runtime
            .resources
            .insert(Arc::new(Mutex::new(DeclaredComponents::default())));
        runtime.resources.insert(catalog);
        runtime.resources.insert(Arc::clone(&reads));
        if let Some(activation) = &activation {
            runtime.resources.insert(Arc::clone(activation));
        }

        let mut session = Self {
            world,
            scripts: ScriptRuntime::new(),
            fuel: PLENTY,
            last_view: None,
            flow: ScriptFlow::default(),
            projected: false,
            runtime: Arc::new(runtime),
            activation,
            reloads,
            events,
            reads,
            took_in: false,
        };
        session.recompile(
            project,
            &project.iter().map(|(m, _)| *m).collect::<Vec<_>>(),
        );
        session.settle();
        session.boundary(TickPhase::Boundary, &mut OutputDeck::new());
        session.boundary(TickPhase::PreExtract, &mut OutputDeck::new());
        session
    }

    /// Requests the scripts on or off, as the editor's Play and Stop do; it
    /// counts from the next frame.
    pub(crate) fn set_scripts(&self, on: bool) {
        let activation = self.activation.as_ref().expect("a switchable session");
        if on {
            activation.activate(khora_core::script::SCRIPT)
        } else {
            activation.deactivate(khora_core::script::SCRIPT)
        }
        .expect("a known family");
    }

    /// The frame boundary, as the SDK runs it: the requests settled, the mail
    /// of a family off for the frame dropped. Whether the scripts are on.
    fn settle(&self) -> bool {
        let Some(activation) = &self.activation else {
            return true;
        };
        activation.settle();
        discard_mail_of_inactive(activation, &self.runtime);
        activation.is_active(khora_core::script::SCRIPT)
    }

    fn boundary(&mut self, phase: TickPhase, deck: &mut OutputDeck) {
        run_data_systems(&mut self.world, &self.runtime, deck, phase);
    }

    /// The flow's view of the world as it is, without running anything.
    pub(crate) fn project(&mut self) -> ScriptView {
        let ctx = FlowContext {
            resumed: !self.projected,
            scope: None,
        };
        self.projected = true;
        let selection = self.flow.select(&self.world, &self.runtime, ctx);
        self.flow.project(&self.world, &selection, &self.runtime)
    }

    /// One frame of `delta` seconds.
    pub(crate) fn frame(&mut self, delta: f32) {
        self.frame_after(delta, &[]);
    }

    /// One frame of `delta` seconds, the deck's command buffer already
    /// holding `before` when the scripts run — what another lane queued
    /// earlier in the tick, which no script sees.
    ///
    /// While the scripts are off, the substrate skips the flow and the
    /// script lane is never executed: the runtime keeps every instance as it
    /// stood.
    pub(crate) fn frame_after(&mut self, delta: f32, before: &[WorldCommand]) {
        let playing = self.settle();
        self.boundary(TickPhase::Boundary, &mut OutputDeck::new());
        self.boundary(TickPhase::PreExtract, &mut OutputDeck::new());
        // What the engine handed over is taken in before the flow projects,
        // while the scripts are on, as the `script_intake` system does at
        // the boundary.
        if playing {
            take_in(
                &mut self.scripts,
                self.reloads.drain(),
                Some(&self.events),
                Some(&self.reads),
                !self.took_in,
            );
        }
        self.took_in = playing;
        if !playing {
            // The substrate skips a flow serving a family off: its next run
            // is a resumption.
            self.projected = false;
        }
        let mut view = if playing {
            self.project()
        } else {
            ScriptView::default()
        };
        view.delta_seconds = delta;
        let mut deck = OutputDeck::new();
        deck.slot::<CommandBuffer>().extend(before.iter().cloned());
        if playing {
            // Where a spawn takes its entity's id, as the flow publishes it.
            let frame = LaneFrame {
                view: &view,
                reservations: khora_data::flow::ReservationView {
                    reserver: self.world.reserver(),
                },
                prefabs: no_prefabs(),
                level: StrategyId::Balanced,
            };
            run_lane(frame, &mut self.scripts, self.fuel, &mut deck);
            self.last_view = Some(view);
        }
        self.boundary(TickPhase::Maintenance, &mut deck);
    }

    /// Every level's price on the last playing frame's view, as the agent
    /// quotes the lane between two frames.
    pub(crate) fn quote(&mut self) -> Vec<(StrategyId, Cost)> {
        let view = self
            .last_view
            .clone()
            .expect("a playing frame ran before the lane is quoted");
        quote_lane(LaneFrame::of(&view), &mut self.scripts)
    }

    /// `modules` recompiled among `project` and handed over the way the
    /// hot-reload pump hands them before the next frame's boundary.
    pub(crate) fn recompile(&self, project: Project, modules: &[&str]) {
        for &module in modules {
            let program = compile(project, module);
            self.reads.announce(
                format!("module:{module}"),
                program
                    .reads
                    .iter()
                    .map(|read| ComponentName::new(read.as_str()))
                    .collect::<Vec<_>>(),
            );
            self.hand_over(module, program);
        }
    }

    fn hand_over(&self, module: &str, program: Program) {
        stage_declarations(
            self.runtime
                .resources
                .get::<Reserve>()
                .expect("the runtime's reserve"),
            declared_in(module, &program),
        );
        self.reloads.send(ScriptReload {
            module: module.to_owned(),
            program: Some(program),
        });
    }
}

/// The identity of the singleton entity of system `name`.
pub(crate) fn singleton_id(name: &str) -> PersistentId {
    PersistentId::named(&format!("system:{name}"))
}

/// The singleton entity of system `name`, when `world` holds one.
pub(crate) fn singleton(world: &World, name: &str) -> Option<EntityId> {
    world
        .entity_with_id(singleton_id(name))
        .filter(|&entity| world.contains(entity))
}

/// The entities holding the declared component `name`, `None` when no
/// component of that name is registered.
pub(crate) fn holders(world: &World, name: &str) -> Option<Vec<EntityId>> {
    world.components().key_named(name)?;
    Some(
        world
            .declared_rows(key(world, name))
            .into_iter()
            .map(|(entity, _)| entity)
            .collect(),
    )
}

/// The one entity holding the component of system `name`.
pub(crate) fn the_holder(world: &World, name: &str) -> EntityId {
    match holders(world, name).as_deref() {
        Some([one]) => *one,
        Some(several) => panic!(
            "one entity holds `{name}`, not {}: {several:?}",
            several.len()
        ),
        None => panic!("`{name}` is registered"),
    }
}
