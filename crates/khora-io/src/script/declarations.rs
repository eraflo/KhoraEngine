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

//! The components a project's scripts declare, registered in the `World`.
//!
//! The compile path — the initial load and the hot-reload pump — sends, for
//! each module it compiled, the components that module declares
//! ([`ScriptDeclarations`]). The `script_components` data system drains them
//! at the frame boundary, where the `World` is mutable, in every engine mode:
//! an editor editing a scene that holds a script's component must know it,
//! though no script runs.
//!
//! What a declaration that changed does to the rows already stored:
//!
//! | The author… | What happens |
//! |---|---|
//! | declares a new component | registered, in its domain |
//! | adds, renames, retypes a field | [`World::relayout`]: kept by name, `int` → `float` converted, the rest defaulted |
//! | changes `[Domain]` | [`World::move_component_domain`]: every row moves, values intact |
//! | deletes the declaration | the component stays, with its data, **orphaned** — never silently deleted |
//! | declares an orphan again | adopted, relaid out to the new declaration |
//!
//! A component another module already declares is refused, its first
//! module's declaration kept: two modules that never import each other each
//! compile alone, so this is the one place that sees both.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use khora_core::event::{Channel, Supersedes, WhenFull};
use khora_core::lane::OutputDeck;
use khora_core::Runtime;
use khora_data::ecs::ScriptRole;
use khora_data::ecs::{
    ComponentProvenance, DataSystemRegistration, FieldKind, PackedField, PackedLayout,
    RuntimeComponentDecl, TickPhase, World,
};
use khora_script::vm::{DeclaredComponent, DeclaredKind};

/// The components one module declares, as it last compiled — none when it
/// declares none, or no longer exists.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptDeclarations {
    /// The module, by its path.
    pub module: String,
    /// What it declares.
    pub components: Vec<DeclaredComponent>,
}

impl Supersedes for ScriptDeclarations {
    const COALESCES: bool = true;

    /// A module's newer set replaces its older one: only what the file says
    /// now is registered.
    fn supersedes(&self, earlier: &Self) -> bool {
        self.module == earlier.module
    }
}

/// Where the compile path leaves what modules declare, for the frame
/// boundary to register.
pub type PendingDeclarations = Channel<ScriptDeclarations>;

/// How many modules' declarations may wait for a frame boundary.
///
/// Drained every frame in every mode, so only the initial load fills it — one
/// set per declaring module, coalesced per module. Large, because a set lost
/// is a component a scene or a script then cannot find.
const DECLARATION_BACKLOG: usize = 1 << 16;

/// A channel for what modules declare.
pub fn declaration_channel() -> PendingDeclarations {
    Channel::bounded(DECLARATION_BACKLOG, WhenFull::Reject)
}

/// What the registration remembers between frames: which module declares
/// each component, and which components no module declares any more.
#[derive(Debug, Default)]
pub struct DeclaredComponents {
    /// Each declared component's module.
    owner: HashMap<String, String>,
    /// Components a module stopped declaring, kept with their data.
    orphans: BTreeSet<String>,
    /// Declarations refused because another module owned the name, by
    /// name: the module and what it declares — its own once the owner stops
    /// declaring it, as a restart would give it.
    refused: HashMap<String, (String, DeclaredComponent)>,
}

impl DeclaredComponents {
    /// Whether `component` is registered with no module declaring it.
    pub fn is_orphaned(&self, component: &str) -> bool {
        self.orphans.contains(component)
    }

    /// The module declaring `component`.
    pub fn owner_of(&self, component: &str) -> Option<&str> {
        self.owner.get(component).map(String::as_str)
    }
}

/// What one [`apply_declarations`] did, each list sorted by name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeclarationReport {
    /// Registered for the first time.
    pub registered: Vec<String>,
    /// Given new fields.
    pub relaid: Vec<String>,
    /// Moved to another domain.
    pub moved: Vec<String>,
    /// No longer declared by any module: kept, with their data.
    pub orphaned: Vec<String>,
    /// Registered with no module declaring them — orphaned, or loaded from a
    /// file — and now declared again.
    pub adopted: Vec<String>,
    /// Declared by a module while another declares them: kept as the first
    /// module declares them.
    pub refused: Vec<String>,
}

/// Registers, relays out, moves and orphans what `batch` declares, in
/// `world`. `state` is the registration's memory across calls.
///
/// A batch is taken whole: a component one module drops and another declares
/// in the same batch changes owner and is never orphaned.
pub fn apply_declarations(
    world: &mut World,
    state: &mut DeclaredComponents,
    batch: &[ScriptDeclarations],
) -> DeclarationReport {
    // A module's last set in the batch is what it says now.
    let mut sets: BTreeMap<&str, &[DeclaredComponent]> = BTreeMap::new();
    for declarations in batch {
        sets.insert(&declarations.module, &declarations.components);
    }
    let mut report = DeclarationReport::default();
    // Who declares what once the batch is taken: a module of the batch says
    // it itself; a module outside it keeps what it owned.
    let mut declared: BTreeMap<&str, (&str, &DeclaredComponent)> = BTreeMap::new();
    for (module, components) in &sets {
        // A behavior refused here takes its state companion with it: the
        // state of a behavior that is not registered is no one's.
        let mut refused_here: Vec<&str> = Vec::new();
        for component in components.iter() {
            let name = component.name.as_str();
            if let DeclaredKind::BehaviorState { behavior } = &component.kind {
                if refused_here.contains(&behavior.as_str()) {
                    report.refused.push(name.to_owned());
                    continue;
                }
            }
            let owner = state.owner.get(name).map(String::as_str).filter(|owner| {
                owner != module
                    && sets
                        .get(owner)
                        .is_none_or(|set| set.iter().any(|kept| kept.name == name))
            });
            let first = owner.or_else(|| {
                declared
                    .get(name)
                    .map(|(first, _)| *first)
                    .filter(|first| first != module)
            });
            if let Some(first) = first {
                state
                    .refused
                    .insert(name.to_owned(), ((*module).to_owned(), component.clone()));
                log::error!(
                    "`{name}` is declared by both `{first}` and `{module}`: it stays as `{first}` declares it — rename one"
                );
                report.refused.push(name.to_owned());
                refused_here.push(name);
                continue;
            }
            declared.insert(name, (module, component));
        }
    }

    // A module of the batch that no longer declares a name it was refused
    // gives up its claim.
    state.refused.retain(|name, (module, _)| {
        sets.get(module.as_str())
            .is_none_or(|set| set.iter().any(|kept| kept.name == *name))
    });

    for (name, (module, component)) in &declared {
        // Sorted by name, a behavior comes before its companion: one that
        // could not be registered leaves its companion unregistered too.
        if let DeclaredKind::BehaviorState { behavior } = &component.kind {
            if world.components().key_named(behavior).is_none() {
                log::error!("`{name}` is not registered: its behavior `{behavior}` is not");
                continue;
            }
        }
        apply_one(world, state, component, module, &mut report);
        state.orphans.remove(*name);
        state.refused.remove(*name);
        state.owner.insert((*name).to_owned(), (*module).to_owned());
    }

    // Whatever a module of the batch owned and no longer declares, unless
    // another module of the batch took it.
    let dropped: Vec<String> = state
        .owner
        .iter()
        .filter(|(name, module)| {
            sets.contains_key(module.as_str()) && !declared.contains_key(name.as_str())
        })
        .map(|(name, _)| name.clone())
        .collect();
    for name in dropped {
        state.owner.remove(&name);
        // A module refused this name still declares it: it is that module's
        // now, not an orphan.
        if let Some((module, component)) = state.refused.remove(&name) {
            apply_one(world, state, &component, &module, &mut report);
            state.owner.insert(name, module);
            continue;
        }
        log::warn!(
            "`{name}` is no longer declared by any script: it stays registered with its data, orphaned"
        );
        state.orphans.insert(name.clone());
        report.orphaned.push(name);
    }

    for list in [
        &mut report.registered,
        &mut report.relaid,
        &mut report.moved,
        &mut report.orphaned,
        &mut report.adopted,
        &mut report.refused,
    ] {
        list.sort();
    }
    report
}

/// Brings one component's registration in line with its declaration, and
/// records the part it plays for scripts.
fn apply_one(
    world: &mut World,
    state: &DeclaredComponents,
    component: &DeclaredComponent,
    module: &str,
    report: &mut DeclarationReport,
) {
    apply_layout(world, state, component, report);
    if let Some(key) = world.components().key_named(&component.name) {
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
    }
}

/// Brings one component's registration in line with its declaration.
fn apply_layout(
    world: &mut World,
    state: &DeclaredComponents,
    component: &DeclaredComponent,
    report: &mut DeclarationReport,
) {
    let name = &component.name;
    let layout = match layout_of(component) {
        Ok(layout) => layout,
        Err(error) => {
            log::error!("`{name}` cannot be stored: {error:?}");
            return;
        }
    };
    let Some(key) = world.components().key_named(name) else {
        // A behavior's state companion is what the engine writes while the
        // behavior runs: never in a scene, always in a game save.
        let state = matches!(component.kind, DeclaredKind::BehaviorState { .. });
        match world.register_runtime_component(RuntimeComponentDecl {
            name: name.clone(),
            domain: component.domain,
            provenance: if state {
                ComponentProvenance::Runtime
            } else {
                ComponentProvenance::Authored
            },
            resumable: state,
            layout,
        }) {
            Ok(_) => report.registered.push(name.clone()),
            Err(error) => log::error!("`{name}` cannot be registered: {error}"),
        }
        return;
    };
    let Some(vtable) = world.components().vtable(key) else {
        return;
    };
    let Some(current) = vtable.columns.packed() else {
        log::error!("`{name}` is a Rust component: a script cannot declare it");
        return;
    };
    if !state.owner.contains_key(name) {
        report.adopted.push(name.clone());
    }
    let (domain, changed) = (vtable.domain, !current.same_as(&layout));
    if changed {
        match world.relayout(key, layout) {
            Ok(_) => report.relaid.push(name.clone()),
            Err(error) => log::error!("`{name}` cannot take its new fields: {error}"),
        }
    }
    if domain != component.domain {
        match world.move_component_domain(key, component.domain) {
            Ok(()) => report.moved.push(name.clone()),
            Err(error) => log::error!("`{name}` cannot move to {:?}: {error}", component.domain),
        }
    }
}

/// The packed layout `component` declares, each field of its Ergon type.
fn layout_of(component: &DeclaredComponent) -> Result<PackedLayout, khora_data::ecs::LayoutError> {
    PackedLayout::typed(
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
}

fn script_components_system(world: &mut World, runtime: &Runtime, _deck: &mut OutputDeck) {
    let Some(pending) = runtime.resources.get::<PendingDeclarations>() else {
        return;
    };
    let batch = pending.drain();
    if batch.is_empty() {
        return;
    }
    let Some(state) = runtime.resources.get::<Arc<Mutex<DeclaredComponents>>>() else {
        log::error!("script components were declared, but the engine keeps no registration state");
        return;
    };
    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
    let report = apply_declarations(world, &mut state, &batch);
    if !report.registered.is_empty() {
        log::info!(
            "script components registered: {}",
            report.registered.join(", ")
        );
    }
}

inventory::submit! {
    DataSystemRegistration {
        name: "script_components",
        phase: TickPhase::PreExtract,
        run: script_components_system,
        order_hint: -9,
        runs_after: &["script_hot_reload"],
    }
}
