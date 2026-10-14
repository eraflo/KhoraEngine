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
//! catalog, the engine's mode, the `PreExtract` data systems, the flow, the
//! script lane run with a chosen fuel, then `Maintenance` applying and
//! writing back.

use std::sync::{Arc, Mutex, RwLock};

use khora_control::substrate::run_data_systems;
use khora_core::agent::{EngineMode, SharedEngineMode};
use khora_core::ecs::entity::EntityId;
use khora_core::ecs::PersistentId;
use khora_core::event::Channel;
use khora_core::lane::{Lane, LaneContext, OutputDeck};
use khora_core::script::{
    engine_event_channel, CommandBuffer, ComponentName, ScriptCatalog, ScriptEvent, WorldCommand,
};
use khora_core::Runtime;
use khora_data::ecs::{TickPhase, World};
use khora_data::flow::{Flow, ScriptFlow, ScriptView};
use khora_io::script::declarations::{declaration_channel, DeclaredComponents};
use khora_lanes::script_lane::{BudgetedScriptLane, Fuel, ScriptRuntime};
use khora_script::reload::ScriptReload;
use khora_script::vm::Program;

use super::behavior_session::{compile, declared_in, key, Project, PLENTY};

/// One running session over a world, in a mode the test chooses.
pub(crate) struct SystemSession {
    pub(crate) world: World,
    pub(crate) scripts: ScriptRuntime,
    /// The fuel each frame's lane runs with.
    pub(crate) fuel: u64,
    flow: ScriptFlow,
    lane: BudgetedScriptLane,
    runtime: Arc<Runtime>,
    mode: SharedEngineMode,
    reloads: Channel<ScriptReload>,
    events: Channel<ScriptEvent>,
    catalog: Arc<ScriptCatalog>,
}

impl SystemSession {
    /// A session over `world` playing `project`.
    pub(crate) fn new(world: World, project: Project) -> Self {
        Self::in_mode(world, project, EngineMode::Playing)
    }

    /// A session over `world`, in `mode`, with `project`'s modules handed
    /// over: every module's declarations registered at once and its program
    /// waiting for the lane, what it reads announced.
    pub(crate) fn in_mode(world: World, project: Project, mode: EngineMode) -> Self {
        let declarations = declaration_channel();
        let reloads = khora_io::script::hot_reload::reload_channel();
        let events = engine_event_channel();
        let catalog = Arc::new(ScriptCatalog::default());
        let mode: SharedEngineMode = Arc::new(RwLock::new(mode));

        let mut runtime = Runtime::default();
        runtime.resources.insert(declarations.clone());
        runtime
            .resources
            .insert(Arc::new(Mutex::new(DeclaredComponents::default())));
        runtime.resources.insert(Arc::clone(&catalog));
        runtime.resources.insert(Arc::clone(&mode));

        let mut session = Self {
            world,
            scripts: ScriptRuntime::new(),
            fuel: PLENTY,
            flow: ScriptFlow::default(),
            lane: BudgetedScriptLane::new(),
            runtime: Arc::new(runtime),
            mode,
            reloads,
            events,
            catalog,
        };
        session.recompile(
            project,
            &project.iter().map(|(m, _)| *m).collect::<Vec<_>>(),
        );
        session.boundary(TickPhase::PreExtract, &mut OutputDeck::new());
        session
    }

    /// Switches the engine's mode, as the editor's Play and Stop do.
    pub(crate) fn set_mode(&self, mode: EngineMode) {
        *self.mode.write().expect("the mode lock") = mode;
    }

    fn boundary(&mut self, phase: TickPhase, deck: &mut OutputDeck) {
        run_data_systems(&mut self.world, &self.runtime, deck, phase);
    }

    /// The flow's view of the world as it is, without running anything.
    pub(crate) fn project(&mut self) -> ScriptView {
        let selection = self.flow.select(&self.world, &self.runtime);
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
    /// The flow projects every frame, as the engine's flows do; the script
    /// lane runs only while the mode plays, because the engine registers the
    /// script agent for `EngineMode::Playing` alone — outside it the lane is
    /// never executed and the runtime keeps every instance as it stood.
    pub(crate) fn frame_after(&mut self, delta: f32, before: &[WorldCommand]) {
        self.boundary(TickPhase::PreExtract, &mut OutputDeck::new());
        let mut view = self.project();
        view.delta_seconds = delta;
        let mut deck = OutputDeck::new();
        deck.slot::<CommandBuffer>().extend(before.iter().cloned());
        let playing = self.mode.read().expect("the mode lock").runs_gameplay();
        if playing {
            let mut ctx = LaneContext::new();
            ctx.insert_ref(&view);
            ctx.insert(Fuel(self.fuel));
            ctx.insert_slot(&mut self.scripts);
            ctx.insert_slot(&mut deck);
            ctx.insert_ref(&self.reloads);
            ctx.insert_ref(&self.events);
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
            self.catalog.announce(
                module,
                program
                    .reads
                    .iter()
                    .map(|read| ComponentName::new(read.as_str()))
                    .collect(),
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
            program,
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
