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

//! A project's systems and behaviors played over a world (Ergon spec 10):
//! one session wired the way the engine wires one — declarations and
//! programs on their channels, what each program reads announced to the
//! catalog, the engine's `Activation` when the test switches the scripts,
//! the `PreExtract` data systems, the flow, the script lane run with a chosen
//! fuel, then `Maintenance` applying and writing back.

use std::sync::{Arc, Mutex};

use khora_control::substrate::run_data_systems;
use khora_core::agent::Activation;
use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;
use khora_core::event::{discard_mail_of_inactive, Channel};
use khora_core::lane::{Lane, LaneContext, OutputDeck};
use khora_core::projection::Interest;
use khora_core::script::{
    engine_event_channel, CommandBuffer, ComponentName, ScriptCatalog, ScriptEvent, ScriptReads,
    WorldCommand,
};
use khora_core::Runtime;
use khora_data::ecs::{TickPhase, World};
use khora_data::flow::{Flow, FlowContext, ScriptFlow, ScriptView};
use khora_io::script::declarations::{declaration_channel, DeclaredComponents};
use khora_lanes::script_lane::{take_in, BudgetedScriptLane, Fuel, ScriptRuntime};
use khora_script::reload::ScriptReload;
use khora_script::vm::Program;

use super::behavior_session::{compile, declared_in, key, Project, PLENTY};

/// One running session over a world, its scripts on or off as the test
/// switches them.
pub(crate) struct SystemSession {
    pub(crate) world: World,
    pub(crate) scripts: ScriptRuntime,
    /// The fuel each frame's lane runs with.
    pub(crate) fuel: u64,
    flow: ScriptFlow,
    /// Whether the flow ran before, as the substrate keeps it: its first run
    /// is a resumption.
    projected: bool,
    lane: BudgetedScriptLane,
    runtime: Arc<Runtime>,
    /// The engine's switch, when the test switches the scripts; without
    /// one, every family is on.
    activation: Option<Arc<Activation>>,
    reloads: Channel<ScriptReload>,
    events: Channel<ScriptEvent>,
    catalog: Arc<ScriptCatalog>,
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
        let declarations = declaration_channel();
        let reloads = khora_io::script::hot_reload::reload_channel();
        let events = engine_event_channel();
        let catalog = Arc::new(ScriptCatalog::default());
        let reads = Arc::new(Interest::<ScriptReads>::new());

        let mut runtime = Runtime::default();
        runtime.resources.insert(declarations.clone());
        runtime
            .resources
            .insert(Arc::new(Mutex::new(DeclaredComponents::default())));
        runtime.resources.insert(Arc::clone(&catalog));
        runtime.resources.insert(Arc::clone(&reads));
        if let Some(activation) = &activation {
            runtime.resources.insert(Arc::clone(activation));
        }

        let mut session = Self {
            world,
            scripts: ScriptRuntime::new(),
            fuel: PLENTY,
            flow: ScriptFlow::default(),
            projected: false,
            lane: BudgetedScriptLane::new(),
            runtime: Arc::new(runtime),
            activation,
            reloads,
            events,
            catalog,
            reads,
            took_in: false,
        };
        session.recompile(
            project,
            &project.iter().map(|(m, _)| *m).collect::<Vec<_>>(),
        );
        session.settle();
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
        self.boundary(TickPhase::PreExtract, &mut OutputDeck::new());
        // What the engine handed over is taken in before the flow projects,
        // while the scripts are on, as the `script_intake` system does at
        // the boundary.
        if playing {
            take_in(
                &mut self.scripts,
                Some(&self.reloads),
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
            let mut ctx = LaneContext::new();
            ctx.insert_ref(&view);
            // Where a spawn takes its entity's id, as the agent hands it on.
            let reservations = khora_data::flow::ReservationView {
                reserver: self.world.reserver(),
            };
            ctx.insert_ref(&reservations);
            ctx.insert(Fuel(self.fuel));
            ctx.insert_slot(&mut self.scripts);
            ctx.insert_slot(&mut deck);
            ctx.insert_ref(&self.catalog);
            self.lane.execute(&mut ctx).expect("the script lane runs");
        }
        self.boundary(TickPhase::Maintenance, &mut deck);
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
        self.runtime
            .resources
            .get::<khora_io::script::declarations::PendingDeclarations>()
            .expect("a declaration channel")
            .send(declared_in(module, &program));
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
