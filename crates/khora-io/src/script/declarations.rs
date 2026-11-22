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

//! The components a project's scripts declare, registered in the `World` at
//! the frame boundary, whether or not scripts run.
//!
//! What a declaration that changed does to the rows already stored:
//!
//! | The author… | What happens |
//! |---|---|
//! | declares a new component | registered, in its domain |
//! | adds, renames, retypes a field | [`World::relayout`]: kept by name where the new type holds the value as a scene load reads it, the rest defaulted; every value lost logged and listed in `Problems` |
//! | changes `[Domain]` | [`World::move_component_domain`]: every row moves, values intact, logged |
//! | deletes the declaration | the component stays, with its data, **orphaned** — never silently deleted |
//! | declares an orphan again | adopted, relaid out to the new declaration |
//!
//! A component another module already declares is refused, the first kept:
//! modules compile alone, so this is the one place that sees both.

use khora_data::ecs::SystemContext;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use khora_core::lane::OutputDeck;
use khora_core::sync::LockExt;
use khora_core::Runtime;
use khora_data::ecs::ScriptRole;
use khora_data::ecs::{
    ComponentProvenance, DataSystemRegistration, FieldKind, PackedField, PackedLayout,
    RelayoutReport, RuntimeComponentDecl, TickPhase, World,
};
use khora_script::modules::behaviors::STATE_SUFFIX;
use khora_script::vm::{DeclaredComponent, DeclaredKind};

use crate::script::declared_relayout::DeclaredRelayout;

/// The components one module declares, as it last compiled.
pub use khora_script::toolchain::ScriptDeclarations;

/// What the registration remembers between frames: which module declares
/// each component, and which components no module declares any more.
#[derive(Debug, Default)]
pub struct DeclaredComponents {
    /// Each declared component's module.
    owner: HashMap<String, String>,
    /// Components a module stopped declaring, kept with their data.
    orphans: BTreeSet<String>,
    /// Declarations refused because another module owned the name, by name
    /// and refused module.
    ///
    /// The first of them inherits the name once its owner stops declaring it,
    /// as a restart would give it.
    refused: BTreeMap<(String, String), DeclaredComponent>,
    /// Modules an unmount closed: what they declared is orphaned with no problem shown.
    unmounted: BTreeSet<String>,
    /// Orphans an unmount left: kept, and nothing to tell anyone about.
    quiet: BTreeSet<String>,
}

impl DeclaredComponents {
    /// Returns whether `component` is registered with no module declaring it.
    pub fn is_orphaned(&self, component: &str) -> bool {
        self.orphans.contains(component)
    }

    /// Returns the module declaring `component`.
    pub fn owner_of(&self, component: &str) -> Option<&str> {
        self.owner.get(component).map(String::as_str)
    }

    /// Returns every name a module declares now, sorted: those a new declaration is refused.
    ///
    /// Components, behaviors and systems; neither a behavior's `#state`
    /// companion nor an orphan.
    pub fn declared_names(&self) -> Vec<khora_core::script::ComponentName> {
        let mut names: Vec<_> = self
            .owner
            .keys()
            .filter(|name| !name.ends_with(STATE_SUFFIX))
            .map(|name| khora_core::script::ComponentName::new(name.as_str()))
            .collect();
        names.sort();
        names
    }

    /// Says `modules` are being unmounted: what they declared is orphaned with no problem shown.
    pub fn unmounting(&mut self, modules: impl IntoIterator<Item = String>) {
        self.unmounted.extend(modules);
    }

    /// Returns what is wrong with the declarations now: the names refused, and the orphans.
    ///
    /// Orphans an unmount left are not listed.
    pub(crate) fn standing_problems(&self) -> Vec<khora_core::problems::Problem> {
        use khora_core::problems::{Problem, ProblemSeverity};
        let refusals = self.refused.keys().map(|(name, module)| {
            let owner = self.owner_of(name).unwrap_or("another module");
            Problem {
                note: Some(
                    "a component's name is unique among every component — rename one".to_owned(),
                ),
                ..Problem::of_file(
                    ProblemSeverity::Error,
                    module.clone(),
                    0,
                    0,
                    format!(
                        "`{name}` is declared by both `{owner}` and `{module}`: it stays as `{owner}` declares it"
                    ),
                )
            }
        });
        let orphans = self
            .orphans
            .iter()
            .filter(|name| !self.quiet.contains(*name))
            .map(|name| Problem {
                note: Some(
                    "declare it again to adopt it, or remove it from the entities holding it"
                        .to_owned(),
                ),
                ..Problem::new(
                    ProblemSeverity::Warning,
                    format!(
                        "`{name}` is no longer declared by any script: it stays registered with its data, orphaned"
                    ),
                )
            });
        refusals.chain(orphans).collect()
    }
}

/// What one [`apply_declarations`] did, each list of names sorted.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeclarationReport {
    /// Registered for the first time.
    pub registered: Vec<String>,
    /// Given new fields.
    pub relaid: Vec<String>,
    /// Moved to another domain.
    pub moved: Vec<String>,
    /// Declared by no module: kept, with their data.
    pub orphaned: Vec<String>,
    /// Registered with no module declaring them — orphaned, or loaded from a
    /// file — and now declared again.
    pub adopted: Vec<String>,
    /// Declared by a module while another declares them: kept as the first
    /// module declares them.
    pub refused: Vec<String>,
    /// Every relayout of stored rows, with each value a row lost, in the
    /// order they ran.
    pub relayouts: Vec<DeclaredRelayout>,
}

/// Registers, relays out, moves and orphans what `batch` declares, in `world`.
///
/// `state` is the registration's memory across calls. A batch is taken whole:
/// a component one module drops and another declares in the same batch
/// changes owner and is never orphaned.
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
                    // Kept with its behavior's refusal: inherited with it.
                    state
                        .refused
                        .insert((name.to_owned(), (*module).to_owned()), component.clone());
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
            let first: Option<String> = owner
                .or_else(|| {
                    declared
                        .get(name)
                        .map(|(first, _)| *first)
                        .filter(|first| first != module)
                })
                .map(str::to_owned)
                // A name freed by this batch goes where a fresh load would
                // give it: to the first module claiming it — one refused it
                // earlier, outside this batch, comes before a later one.
                .or_else(|| {
                    state
                        .refused
                        .keys()
                        .find(|(refused, earlier)| {
                            refused == name
                                && earlier.as_str() < *module
                                && !sets.contains_key(earlier.as_str())
                        })
                        .map(|(_, earlier)| earlier.clone())
                });
            if let Some(first) = first {
                state
                    .refused
                    .insert((name.to_owned(), (*module).to_owned()), component.clone());
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

    // A module of the batch that stops declaring a name it was refused
    // gives up its claim.
    state.refused.retain(|(name, module), _| {
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
        state.quiet.remove(*name);
        // Its own refusal, if it had one, is over; another module's refusal
        // of the same name stands — refused by this very batch, perhaps.
        state
            .refused
            .remove(&((*name).to_owned(), (*module).to_owned()));
        state.owner.insert((*name).to_owned(), (*module).to_owned());
    }

    // Whatever a module of the batch owned and stops declaring, unless
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
        let closed = state
            .owner
            .remove(&name)
            .is_some_and(|module| state.unmounted.contains(&module));
        // A module refused this name still declares it: it is that module's
        // now, not an orphan — the first of them, by module.
        let heir = state
            .refused
            .keys()
            .find(|(refused, _)| *refused == name)
            .cloned();
        if let Some(key) = heir {
            if let Some(component) = state.refused.remove(&key) {
                let module = key.1;
                apply_one(world, state, &component, &module, &mut report);
                state.owner.insert(name, module);
                continue;
            }
        }
        if closed {
            state.quiet.insert(name.clone());
        }
        log::warn!(
            "`{name}` is no longer declared by any script: it stays registered with its data, orphaned"
        );
        if let Some(key) = world.components().key_named(&name) {
            world.set_declared_by(key, None);
        }
        state.orphans.insert(name.clone());
        report.orphaned.push(name);
    }

    // A behavior inherited takes along its companion, refused with it.
    let companions: Vec<(String, String)> = state
        .refused
        .iter()
        .filter_map(|((name, module), component)| match &component.kind {
            DeclaredKind::BehaviorState { behavior }
                if state.owner.get(behavior) == Some(module) =>
            {
                Some((name.clone(), module.clone()))
            }
            _ => None,
        })
        .collect();
    for key in companions {
        if let Some(component) = state.refused.remove(&key) {
            let (name, module) = key;
            apply_one(world, state, &component, &module, &mut report);
            state.orphans.remove(&name);
            state.quiet.remove(&name);
            if let Some(key) = world.components().key_named(&name) {
                world.set_declared_by(key, Some(module.clone()));
            }
            state.owner.insert(name, module);
        }
    }

    // A module of the batch declaring something is mounted again.
    for (module, components) in &sets {
        if !components.is_empty() {
            state.unmounted.remove(*module);
        }
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
    apply_layout(world, state, component, module, report);
    if let Some(key) = world.components().key_named(&component.name) {
        let role = match &component.kind {
            DeclaredKind::Component => None,
            DeclaredKind::Behavior => Some(ScriptRole::Behavior {
                module: module.to_owned(),
                order: component.order,
            }),
            DeclaredKind::System => Some(ScriptRole::System {
                module: module.to_owned(),
                order: component.order,
            }),
            DeclaredKind::BehaviorState { behavior } => Some(ScriptRole::BehaviorState {
                behavior: behavior.clone(),
            }),
        };
        world.set_script_role(key, role);
        world.set_declared_by(key, Some(module.to_owned()));
        // Every reload, whether or not the layout changed: a hint is not
        // the layout, and editing one never relays the component out.
        world.set_field_hints(
            key,
            component
                .fields
                .iter()
                .filter(|field| !field.hints.is_empty())
                .map(|field| (field.name.clone(), field.hints.clone()))
                .collect(),
        );
    }
}

/// Brings one component's registration in line with its declaration, as `module` declares it.
///
/// Logs each relayout and domain change, and keeps each relayout in `report`.
fn apply_layout(
    world: &mut World,
    state: &DeclaredComponents,
    component: &DeclaredComponent,
    module: &str,
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
    // A behavior's state companion is what the engine writes while the
    // behavior runs, and a system's fields are its own entity's: never in a
    // scene, always in a game save.
    let engine_kept = matches!(
        component.kind,
        DeclaredKind::BehaviorState { .. } | DeclaredKind::System
    );
    let provenance = if engine_kept {
        ComponentProvenance::Runtime
    } else {
        ComponentProvenance::Authored
    };
    let Some(key) = world.components().key_named(name) else {
        match world.register_runtime_component(RuntimeComponentDecl {
            name: name.clone(),
            domain: component.domain,
            provenance,
            resumable: engine_kept,
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
    if vtable.columns.packed().is_none() {
        log::error!("`{name}` is a Rust component: a script cannot declare it");
        return;
    }
    if !state.owner.contains_key(name) {
        report.adopted.push(name.clone());
    }
    // A name that changed kind — a behavior become a system — is kept as its
    // new kind is.
    if vtable.provenance != provenance || vtable.resumable != engine_kept {
        match world.set_provenance(key, provenance, engine_kept) {
            Ok(relaid) => keep_relayout(report, module, relaid),
            Err(error) => log::error!("`{name}` cannot change what keeps it: {error}"),
        }
    }
    let Some(vtable) = world.components().vtable(key) else {
        return;
    };
    let Some(current) = vtable.columns.packed() else {
        return;
    };
    let (domain, changed) = (vtable.domain, !current.same_as(&layout));
    if changed {
        match world.relayout(key, layout) {
            Ok(relaid) => {
                report.relaid.push(name.clone());
                keep_relayout(report, module, relaid);
            }
            Err(error) => log::error!("`{name}` cannot take its new fields: {error}"),
        }
    }
    if domain != component.domain {
        match world.move_component_domain(key, component.domain) {
            Ok(()) => {
                log::info!(
                    "`{name}` moved from {domain:?} to {:?}, every value intact",
                    component.domain
                );
                report.moved.push(name.clone());
            }
            Err(error) => log::error!("`{name}` cannot move to {:?}: {error}", component.domain),
        }
    }
}

/// Logs what relaying out a component of `module` did, and keeps it in `report`.
fn keep_relayout(report: &mut DeclarationReport, module: &str, relaid: RelayoutReport) {
    let relayout = DeclaredRelayout {
        module: module.to_owned(),
        report: relaid,
    };
    relayout.log();
    report.relayouts.push(relayout);
}

/// Returns the packed layout `component` declares, each field of its Ergon type.
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

/// Registers in `world` what the compile path staged in `runtime`'s
/// [`Reserve`](khora_core::lane::Reserve).
///
/// The runtime's `Arc<Mutex<DeclaredComponents>>` is the registration's
/// memory. Takes what the reserve holds; an empty report when the runtime
/// holds no reserve, no state, or nothing is staged.
///
/// Reports to the runtime's `Problems`, when it holds them, what the
/// registration refuses or orphans (`ProblemSource::Registration`), and for
/// each module whose components were relaid out, every value its rows lost
/// (`ProblemSource::Relayout`): once per relayout, standing until the
/// module's next one, and only while the module declares the component.
pub fn register_pending(world: &mut World, runtime: &Runtime) -> DeclarationReport {
    let Some(reserve) = runtime.look_up::<khora_core::lane::Reserve>() else {
        return DeclarationReport::default();
    };
    let batch = reserve.take::<ScriptDeclarations>();
    if batch.is_empty() {
        return DeclarationReport::default();
    }
    let Some(state) = runtime.resources.get::<Arc<Mutex<DeclaredComponents>>>() else {
        log::error!("script components were declared, but the engine keeps no registration state");
        return DeclarationReport::default();
    };
    let mut state = state.lock_recovered();
    let report = apply_declarations(world, &mut state, &batch);
    if let Some(problems) = runtime
        .resources
        .get::<Arc<khora_core::problems::Problems>>()
    {
        problems.report(
            khora_core::problems::ProblemSource::Registration,
            state.standing_problems(),
        );
        report_relayouts(problems, &state, &batch, &report.relayouts);
    }
    // The tools that cannot see the registry read what it lists here.
    if let Some(catalog) = runtime
        .resources
        .get::<Arc<khora_core::script::ScriptCatalog>>()
    {
        catalog.set_declared(state.declared_names());
    }
    if !report.registered.is_empty() {
        log::info!(
            "script components registered: {}",
            report.registered.join(", ")
        );
    }
    report
}

/// Reports each module's relayout warnings: only about components the module still declares.
///
/// A module that relaid out in this batch reports what that relayout lost,
/// replacing what an earlier one reported. Any other module of the batch, or
/// declaring a component it inherited, keeps its warnings about what it still
/// declares: a module deleted or unmounted keeps none, and one that gave a
/// component to another keeps none about it.
fn report_relayouts(
    problems: &khora_core::problems::Problems,
    state: &DeclaredComponents,
    batch: &[ScriptDeclarations],
    relayouts: &[DeclaredRelayout],
) {
    use khora_core::problems::{Problem, ProblemSource};
    let mut relaid: BTreeMap<&str, Vec<Problem>> = BTreeMap::new();
    for relayout in relayouts {
        relaid
            .entry(relayout.module.as_str())
            .or_default()
            .extend(relayout.problems());
    }
    let modules: BTreeSet<&str> = batch
        .iter()
        .map(|declarations| declarations.module.as_str())
        .chain(relaid.keys().copied())
        .collect();
    for module in modules {
        let source = ProblemSource::Relayout {
            module: module.to_owned(),
        };
        let standing = relaid
            .remove(module)
            .unwrap_or_else(|| problems.of_source(&source));
        let declared = standing
            .into_iter()
            .filter(|problem| {
                problem
                    .component
                    .as_deref()
                    .is_some_and(|component| state.owner_of(component) == Some(module))
            })
            .collect();
        problems.report(source, declared);
    }
}

fn script_components_system(
    world: &mut World,
    runtime: &Runtime,
    _deck: &mut OutputDeck,
    _ctx: SystemContext,
) {
    register_pending(world, runtime);
}

// At the frame boundary, whatever families are on: an editor editing a
// scene registers what a script it saves declares.
inventory::submit! {
    DataSystemRegistration {
        name: "script_components",
        phase: TickPhase::Boundary,
        run: script_components_system,
        order_hint: 0,
        runs_after: &[],
        observes: &[],
        role: khora_data::ecs::Role::Upkeep,
    }
}
