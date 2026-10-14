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

//! Worlds whose behaviors are components (Ergon spec 09), for the tests that
//! take the road a frame takes: the flow projects each entity holding `Guard`
//! with its row, the lane runs it, and the engine's writeback records what the
//! turn changed into `Guard` and what it keeps of the instance into
//! `Guard#state`.
//!
//! The scripts' registration lives in `khora-io`, above this crate; what it
//! registers — each declared component, its provenance, its role for scripts
//! and the module declaring it — is registered here the same way.

use khora_core::ecs::entity::EntityId;
use khora_core::lane::OutputDeck;
use khora_core::script::{BehaviorWriteback, ScriptSnapshot, ScriptValue};
use khora_core::Runtime;
use khora_data::ecs::{
    ComponentKey, ComponentProvenance, DataSystemRegistration, FieldKind, PackedField,
    PackedLayout, RuntimeComponentDecl, ScriptRole, World,
};
use khora_data::flow::companion::{companion_row, companion_snapshot};
use khora_lanes::script_lane::{ScriptRunReport, ScriptRuntime};
use khora_script::vm::{DeclaredComponent, DeclaredKind, Program};
use khora_script::{compile_module, Diagnostic, MemoryLoader};

/// What separates a behavior's name from its state companion's.
const STATE_SUFFIX: &str = "#state";

/// `source`, compiled as the module `module` — with what it declares.
pub(crate) fn program(module: &str, source: &str) -> Program {
    let result = compile_module(&MemoryLoader::new().with(module, source), module);
    let errors: Vec<&Diagnostic> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(errors.is_empty(), "`{module}` should compile: {errors:?}");
    result.program.expect("a program with no errors")
}

/// A world declaring what `module`'s `source` declares, and a script runtime
/// holding its program.
pub(crate) fn declared(module: &str, source: &str) -> (World, ScriptRuntime) {
    let program = program(module, source);
    let mut world = World::new();
    declare(&mut world, module, &program);
    let mut runtime = ScriptRuntime::new();
    runtime.add_program(module, program);
    (world, runtime)
}

/// Registers every component `program` declares, as the scripts'
/// registration does: a behavior's component authored, its state companion
/// runtime and resumable, each with its role and its module.
pub(crate) fn declare(world: &mut World, module: &str, program: &Program) {
    for component in &program.declared {
        register(world, module, component);
    }
}

fn register(world: &mut World, module: &str, component: &DeclaredComponent) {
    let state = matches!(component.kind, DeclaredKind::BehaviorState { .. });
    let layout = PackedLayout::typed(
        component
            .fields
            .iter()
            .map(|field| PackedField {
                name: field.name.clone(),
                kind: FieldKind::of(&field.ty),
                default: field.default.clone(),
            })
            .collect(),
        component
            .fields
            .iter()
            .map(|field| field.ty.clone())
            .collect(),
    )
    .unwrap_or_else(|e| panic!("`{}` has a layout: {e:?}", component.name));
    let key = world
        .register_runtime_component(RuntimeComponentDecl {
            name: component.name.clone(),
            domain: component.domain,
            provenance: if state {
                ComponentProvenance::Runtime
            } else {
                ComponentProvenance::Authored
            },
            resumable: state,
            layout,
        })
        .unwrap_or_else(|e| panic!("`{}` registers: {e}", component.name));
    let role = match &component.kind {
        DeclaredKind::Component => None,
        DeclaredKind::Behavior => Some(ScriptRole::Behavior {
            module: module.to_owned(),
        }),
        DeclaredKind::BehaviorState { behavior } => Some(ScriptRole::BehaviorState {
            behavior: behavior.clone(),
        }),
    };
    world.set_script_role(key, role);
    world.set_declared_by(key, Some(module.to_owned()));
}

/// The key of the component named `name`.
pub(crate) fn key(world: &World, name: &str) -> ComponentKey {
    world
        .components()
        .key_named(name)
        .unwrap_or_else(|| panic!("`{name}` is registered"))
}

/// Attaches the component `name` to `entity`, its named fields set and the
/// others at their declared defaults — what adding it in the editor does.
pub(crate) fn attach(
    world: &mut World,
    entity: EntityId,
    name: &str,
    fields: &[(String, ScriptValue)],
) {
    let key = key(world, name);
    world
        .add_runtime_component(entity, key, &ScriptValue::Struct(fields.to_vec()))
        .unwrap_or_else(|e| panic!("`{name}` attaches: {e:?}"));
}

/// Gives `entity` what a game save holds of its `behavior` running as
/// `observed` says: the behavior's component with the fields it had, and its
/// state companion.
pub(crate) fn restore(
    world: &mut World,
    entity: EntityId,
    behavior: &str,
    observed: &ScriptSnapshot,
) {
    attach(world, entity, behavior, &observed.fields);
    restore_state(world, entity, behavior, observed);
}

/// Gives `entity` the state companion of `behavior` that `observed` records.
pub(crate) fn restore_state(
    world: &mut World,
    entity: EntityId,
    behavior: &str,
    observed: &ScriptSnapshot,
) {
    let row = companion_row(observed).expect("a snapshot fills a companion");
    attach(world, entity, &format!("{behavior}{STATE_SUFFIX}"), &row);
}

/// What the world holds of `entity`'s `behavior`: the behavior's fields and
/// its state companion, read back as a snapshot. `None` before the companion
/// was written.
pub(crate) fn recorded(world: &World, entity: EntityId, behavior: &str) -> Option<ScriptSnapshot> {
    let companion = world.declared_row(entity, key(world, &format!("{behavior}{STATE_SUFFIX}")))?;
    let mut snapshot = companion_snapshot(&companion).expect("a companion reads back");
    snapshot.fields = world.declared_row(entity, key(world, behavior))?;
    Some(snapshot)
}

/// Writes what the lane reported back onto the world through the engine's
/// own `behavior_writeback`, as the frame's `Maintenance` runs it.
pub(crate) fn write_back(world: &mut World, report: &ScriptRunReport) {
    let mut deck = OutputDeck::new();
    deck.slot::<BehaviorWriteback>()
        .extend(report.state.iter().cloned());
    let system = khora_script::inventory::iter::<DataSystemRegistration>
        .into_iter()
        .find(|system| system.name == "behavior_writeback")
        .expect("the engine registers the behaviors' writeback");
    (system.run)(world, &Runtime::default(), &mut deck);
}
