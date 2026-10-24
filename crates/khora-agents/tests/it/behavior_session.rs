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

//! A project's behaviors played as components (Ergon spec 09): one session
//! wired the way the engine wires one — declarations and programs on their
//! channels, the `PreExtract` registration, the flow, the script lane run
//! with a chosen fuel, then `Maintenance` applying and writing back.

use std::sync::{Arc, Mutex};

use khora_control::substrate::run_data_systems;
use khora_core::ecs::entity::EntityId;
use khora_core::event::Channel;
use khora_core::lane::{Lane, LaneContext, OutputDeck};
use khora_core::script::{engine_event_channel, CommandBuffer, ScriptCatalog, ScriptEvent};
use khora_core::script::{ScriptValue, WorldCommand};
use khora_core::Runtime;
use khora_data::ecs::{ComponentKey, TickPhase, World};
use khora_data::flow::{Flow, ScriptFlow, ScriptView};
use khora_io::script::declarations::{declaration_channel, DeclaredComponents, ScriptDeclarations};
use khora_io::script::mirror::PreludeLoader;
use khora_lanes::script_lane::{BudgetedScriptLane, Fuel, ScriptRuntime};
use khora_script::reload::ScriptReload;
use khora_script::vm::Program;
use khora_script::{compile_module, Diagnostic, MemoryLoader};

/// Far more fuel than any frame of these tests spends.
pub(crate) const PLENTY: u64 = 10_000_000;

/// A project: each module's path and source.
pub(crate) type Project = &'static [(&'static str, &'static str)];

/// `module` compiled among the project's modules, the mirrors available.
pub(crate) fn compile(project: Project, module: &str) -> Program {
    let mut inner = MemoryLoader::new();
    for &(path, source) in project {
        inner = inner.with(path, source);
    }
    let result = compile_module(&PreludeLoader::new(inner), module);
    let errors: Vec<&Diagnostic> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "`{module}` should compile: {errors:?}");
    result.program.expect("a program with no errors")
}

/// What the engine sends for `module`'s declarations.
fn declarations_of(project: Project, module: &str) -> ScriptDeclarations {
    declared_in(module, &compile(project, module))
}

/// What the engine sends for the components `program`, compiled as
/// `module`, declares in that module — the ones it declares through an
/// import are that module's to send.
pub(crate) fn declared_in(module: &str, program: &Program) -> ScriptDeclarations {
    ScriptDeclarations {
        module: module.to_owned(),
        components: program
            .declared
            .iter()
            .filter(|declared| declared.module == module)
            .cloned()
            .collect(),
    }
}

/// One running session over a world.
pub(crate) struct Session {
    pub(crate) world: World,
    pub(crate) scripts: ScriptRuntime,
    /// The fuel each frame's lane runs with.
    pub(crate) fuel: u64,
    flow: ScriptFlow,
    lane: BudgetedScriptLane,
    runtime: Arc<Runtime>,
    reloads: Channel<ScriptReload>,
    events: Channel<ScriptEvent>,
    catalog: Arc<ScriptCatalog>,
}

impl Session {
    /// A session over `world` playing `project`: every module's declarations
    /// registered at once — so components can be attached before the first
    /// frame — and every module's program waiting for the lane.
    pub(crate) fn new(world: World, project: Project) -> Self {
        Self::with_runtime(world, project, |_| {})
    }

    /// [`Session::new`], with `prepare` adding to the runtime what the
    /// engine's bootstrap would — a prefab library, a prefab catalog.
    pub(crate) fn with_runtime(
        world: World,
        project: Project,
        prepare: impl FnOnce(&mut Runtime),
    ) -> Self {
        let declarations = declaration_channel();
        let reloads = khora_io::script::hot_reload::reload_channel();
        let events = engine_event_channel();
        let catalog = Arc::new(ScriptCatalog::default());

        let mut runtime = Runtime::default();
        runtime.resources.insert(declarations.clone());
        runtime
            .resources
            .insert(Arc::new(Mutex::new(DeclaredComponents::default())));
        runtime.resources.insert(Arc::clone(&catalog));
        prepare(&mut runtime);

        let mut session = Self {
            world,
            scripts: ScriptRuntime::new(),
            fuel: PLENTY,
            flow: ScriptFlow::default(),
            lane: BudgetedScriptLane::new(),
            runtime: Arc::new(runtime),
            reloads,
            events,
            catalog,
        };
        for &(module, _) in project {
            declarations.send(declarations_of(project, module));
            session.reloads.send(ScriptReload {
                module: module.to_owned(),
                program: Some(compile(project, module)),
            });
        }
        session.boundary(TickPhase::PreExtract, &mut OutputDeck::new());
        session
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
        self.frame_with(delta, |_| {});
    }

    /// One frame of `delta` seconds, `also` queuing commands beside the
    /// scripts' — what another script running the same frame would have
    /// asked for.
    pub(crate) fn frame_with(&mut self, delta: f32, also: impl FnOnce(&mut CommandBuffer)) {
        self.boundary(TickPhase::PreExtract, &mut OutputDeck::new());
        let mut view = self.project();
        view.delta_seconds = delta;
        let mut deck = OutputDeck::new();
        {
            let mut ctx = LaneContext::new();
            ctx.insert_ref(&view);
            // Where a spawn takes its entity's id, as the agent hands it on.
            let reservations = khora_data::flow::ReservationView {
                reserver: self.world.reserver(),
            };
            ctx.insert_ref(&reservations);
            // The prefabs a spawn may name, as the agent hands them on: the
            // runtime's catalog, or none to check against.
            let prefabs = khora_data::flow::PrefabCatalogView {
                prefabs: self
                    .runtime
                    .resources
                    .get::<khora_data::scene::PrefabCatalog>()
                    .map(khora_data::scene::PrefabCatalog::snapshot),
            };
            ctx.insert_ref(&prefabs);
            ctx.insert(Fuel(self.fuel));
            ctx.insert_slot(&mut self.scripts);
            ctx.insert_slot(&mut deck);
            ctx.insert_ref(&self.reloads);
            ctx.insert_ref(&self.events);
            ctx.insert_ref(&self.catalog);
            self.lane.execute(&mut ctx).expect("the script lane runs");
        }
        also(deck.slot::<CommandBuffer>());
        self.boundary(TickPhase::Maintenance, &mut deck);
    }

    /// Raises `event` from the engine, for the next frame to deliver.
    pub(crate) fn send(&self, event: ScriptEvent) {
        self.events.send(event);
    }

    /// `modules` saved by their author, recompiled among `project`, handed
    /// over the way the hot-reload pump hands them before the next frame's
    /// boundary: what each declares, what its program reads announced to the
    /// catalog, and the program.
    pub(crate) fn recompile(&self, project: Project, modules: &[&str]) {
        for &module in modules {
            let program = compile(project, module);
            self.catalog.announce(
                module,
                program
                    .reads
                    .iter()
                    .map(|read| khora_core::script::ComponentName::new(read.as_str()))
                    .collect(),
            );
            self.hand_over(module, program);
        }
    }

    /// `modules` recompiled among `project` and sent straight into the
    /// channels, announcing nothing — a program installed some other way
    /// than through the pump.
    pub(crate) fn recompile_unannounced(&self, project: Project, modules: &[&str]) {
        for &module in modules {
            self.hand_over(module, compile(project, module));
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

    /// The catalog the lane writes and the flow reads.
    pub(crate) fn catalog(&self) -> &ScriptCatalog {
        &self.catalog
    }
}

/// The key of the component named `name`.
pub(crate) fn key(world: &World, name: &str) -> ComponentKey {
    world
        .components()
        .key_named(name)
        .unwrap_or_else(|| panic!("`{name}` is registered"))
}

/// Attaches the component `name` to `entity`, its named fields set and the
/// others at their declared defaults — what adding it in the editor, or
/// `Add` from a script, does.
pub(crate) fn attach(
    world: &mut World,
    entity: EntityId,
    name: &str,
    fields: &[(&str, ScriptValue)],
) {
    let key = key(world, name);
    let value = ScriptValue::Struct(
        fields
            .iter()
            .map(|(field, value)| ((*field).to_owned(), value.clone()))
            .collect(),
    );
    world
        .add_runtime_component(entity, key, &value)
        .unwrap_or_else(|e| panic!("`{name}` attaches: {e:?}"));
}

/// Whether `entity` holds the component `name`.
pub(crate) fn holds(world: &World, entity: EntityId, name: &str) -> bool {
    world.row(entity, key(world, name)).is_some()
}

/// Field `field` of `entity`'s `name`, as the `World` holds it.
pub(crate) fn field(
    world: &World,
    entity: EntityId,
    name: &str,
    field: &str,
) -> Option<ScriptValue> {
    let key = key(world, name);
    let slot = world
        .components()
        .vtable(key)
        .and_then(|vtable| vtable.columns.packed())
        .and_then(|packed| packed.slot_of(field))
        .unwrap_or_else(|| panic!("`{name}` has a field `{field}`"));
    world.row(entity, key)?.field(slot)
}

/// Field `field` of `entity`'s `name`, as an `int`.
pub(crate) fn int(world: &World, entity: EntityId, name: &str, name_of_field: &str) -> Option<i64> {
    field(world, entity, name, name_of_field)?.as_int()
}

/// Writes field `field` of `entity`'s `name` in place — what the inspector
/// does to a component during Play.
pub(crate) fn edit(
    world: &mut World,
    entity: EntityId,
    name: &str,
    field: &str,
    value: ScriptValue,
) {
    let key = key(world, name);
    let slot = world
        .components()
        .vtable(key)
        .and_then(|vtable| vtable.columns.packed())
        .and_then(|packed| packed.slot_of(field))
        .unwrap_or_else(|| panic!("`{name}` has a field `{field}`"));
    world
        .row_mut(entity, key)
        .unwrap_or_else(|| panic!("the entity holds `{name}`"))
        .set_field(slot, &value)
        .unwrap_or_else(|e| panic!("`{name}.{field}` takes {value:?}: {e:?}"));
}

/// The patch another script's `e.Set(name { field: value })` queues.
pub(crate) fn set_patch(
    entity: EntityId,
    name: &str,
    field: &str,
    value: ScriptValue,
) -> WorldCommand {
    WorldCommand::SetComponent {
        entity,
        component: name.into(),
        value: ScriptValue::Struct(vec![(field.to_owned(), value)]),
    }
}
