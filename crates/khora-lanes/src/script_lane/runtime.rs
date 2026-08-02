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

//! What scripting keeps between frames.
//!
//! The compiled programs and each instance's fields — everything that outlives a
//! frame but is not part of the scene. It is deliberately **not** in the
//! `World`: a lane may not touch that (`RULES.md` §3), and putting the fields
//! there would mean either the lane writing them or the whole store crossing the
//! boundary twice a frame.
//!
//! Nor is it shared. The agent owns one of these outright, the way it owns its
//! lane registry, so the script lane touches nothing another agent can see —
//! which is what lets the agent declare [`AgentAccess::Isolated`] honestly
//! rather than by assertion.
//!
//! [`AgentAccess::Isolated`]: khora_core::agent::AgentAccess::Isolated

use std::collections::HashMap;

use khora_core::ecs::entity::EntityId;
use khora_script::arena::{Persisted, PersistentStore};
use khora_script::vm::Value;
use khora_script::vm::{BehaviorLayout, Program};

/// One behavior instance's state.
#[derive(Debug, Default)]
pub struct Instance {
    /// Its fields.
    pub fields: PersistentStore,
    /// Whether its declared defaults have been applied.
    ///
    /// A default is an expression, so it takes a run to produce; this is what
    /// stops that run happening again every frame.
    pub initialised: bool,
    /// Whether the instance has faulted and stopped being called.
    ///
    /// A faulty script must not take the frame down, and must not be retried
    /// forever either — a behavior that faults every frame would fill the log
    /// and spend the budget doing it.
    pub disabled: bool,

    /// Values carried across a reload, to be restored **after** the new
    /// program's initialiser has run.
    ///
    /// Not simply left in [`fields`](Self::fields), because the initialiser
    /// writes every slot: a behavior that gained a field has to run it for the
    /// new slot to get its declared default, and that run would overwrite the
    /// values the reload just carried. So they are set aside and put back on
    /// top — the new field gets its default, the old ones keep what they had.
    pub carried: Option<PersistentStore>,
}

/// What a reload did to one behavior's instances.
///
/// Reported rather than logged here, so the caller decides where it goes — the
/// editor's Console while playing, the terminal for a headless run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadReport {
    /// Which behavior.
    pub behavior: String,
    /// Fields that survived the edit with their values.
    pub kept: Vec<String>,
    /// Fields the edit introduced, which take their declared defaults.
    pub added: Vec<String>,
    /// Fields the edit removed, whose values are gone.
    ///
    /// The one an author most wants to hear about: a renamed field looks like
    /// one dropped and one added, and its value did not travel.
    pub dropped: Vec<String>,
}

impl ReloadReport {
    /// Whether anything was lost.
    pub fn lost_anything(&self) -> bool {
        !self.dropped.is_empty()
    }
}

/// Moves an instance's values from the old slot layout to the new one.
///
/// By name, never by position: inserting one field at the top shifts every slot
/// after it, and carrying values positionally would move a guard's health into
/// its speed without a word.
fn remap(current: &PersistentStore, old: &BehaviorLayout, new: &BehaviorLayout) -> PersistentStore {
    let mut next = PersistentStore::with_slots(new.fields.len());
    for (slot, field) in new.fields.iter().enumerate() {
        let Some(was) = old.slot_of(field) else {
            // New field: left unset, because the initialiser will fill it and
            // a zero written here would shadow the author's declared default.
            continue;
        };
        if let Some(value) = current.get(was) {
            next.set(slot, value.clone());
        }
    }
    next
}

/// Writes `carried` over `fields`, skipping the slots it left unset.
///
/// The second half of a reload: the initialiser has just produced the new
/// program's defaults for every slot, and this puts back the values that
/// survived the edit. Unset slots are skipped rather than copied, which is
/// exactly how a newly-added field keeps the default it was just given.
pub fn restore_carried(fields: &mut PersistentStore, carried: &PersistentStore) {
    for slot in 0..carried.len() {
        match carried.get(slot) {
            Some(Persisted::Scalar(Value::Unit)) | None => {}
            Some(value) => fields.set(slot, value.clone()),
        }
    }
}

/// Compiled programs and live instances.
#[derive(Debug, Default)]
pub struct ScriptRuntime {
    programs: HashMap<String, Program>,
    instances: HashMap<(EntityId, String), Instance>,
}

impl ScriptRuntime {
    /// An empty runtime.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a compiled module under the path it came from.
    pub fn add_program(&mut self, module: impl Into<String>, program: Program) {
        self.programs.insert(module.into(), program);
    }

    /// Replaces a module, carrying every live instance's state across.
    ///
    /// The point of hot-reload, and the part that is not plumbing: an author
    /// edits a guard's `Update` while ten guards are patrolling, and the ten
    /// should still be where they were, with the health they had. Restarting
    /// them would make the feature useless for exactly the thing it is for.
    ///
    /// Fields are matched **by name**, because a slot means nothing across a
    /// recompile — inserting one field at the top shifts every slot after it,
    /// and a positional carry-over would silently move a guard's health into
    /// its speed. A field that survived the edit keeps its value; one that is
    /// new is left for the behavior's initialiser to fill; one that is gone is
    /// dropped.
    ///
    /// Returns what changed, so the caller can say so rather than reloading in
    /// silence — an author who renamed a field and lost its value should be
    /// told, not left to discover it.
    pub fn reload(&mut self, module: &str, program: Program) -> Vec<ReloadReport> {
        let previous = self.programs.get(module).cloned();
        let mut reports = Vec::new();

        for layout in &program.behaviors {
            let old = previous
                .as_ref()
                .and_then(|program| program.layout(&layout.name));
            let Some(old) = old else {
                // A behavior the module did not have before: nothing to carry.
                continue;
            };

            let report = ReloadReport {
                behavior: layout.name.clone(),
                kept: layout
                    .fields
                    .iter()
                    .filter(|field| old.slot_of(field).is_some())
                    .cloned()
                    .collect(),
                added: layout
                    .fields
                    .iter()
                    .filter(|field| old.slot_of(field).is_none())
                    .cloned()
                    .collect(),
                dropped: old
                    .fields
                    .iter()
                    .filter(|field| layout.slot_of(field).is_none())
                    .cloned()
                    .collect(),
            };

            for ((_, behavior), instance) in self.instances.iter_mut() {
                if behavior != &layout.name {
                    continue;
                }
                let carried = remap(&instance.fields, old, layout);
                instance.fields = carried.clone();
                // Re-initialised so the new program's defaults are produced —
                // its literals may have changed too, not only its field list.
                // The carried values go back on top afterwards.
                instance.initialised = false;
                instance.carried = Some(carried);
                // An edit is the author's answer to whatever faulted. Refusing
                // to try again would make a script unfixable without a restart.
                instance.disabled = false;
            }

            reports.push(report);
        }

        self.programs.insert(module.to_owned(), program);
        reports
    }

    /// The program compiled from `module`.
    pub fn program(&self, module: &str) -> Option<&Program> {
        self.programs.get(module)
    }

    /// How many modules are loaded.
    pub fn len(&self) -> usize {
        self.programs.len()
    }

    /// Whether none are.
    pub fn is_empty(&self) -> bool {
        self.programs.is_empty()
    }

    /// The state of one entity's behavior, created empty if it is new.
    pub fn instance(&mut self, entity: EntityId, behavior: &str) -> &mut Instance {
        self.instances
            .entry((entity, behavior.to_owned()))
            .or_default()
    }

    /// The state of one entity's behavior, if it has any.
    pub fn peek(&self, entity: EntityId, behavior: &str) -> Option<&Instance> {
        self.instances.get(&(entity, behavior.to_owned()))
    }

    /// Forgets every instance the given predicate does not keep.
    ///
    /// Called with the frame's live entities, which is how a despawned one's
    /// fields are released. Sweeping rather than reacting to a despawn: the
    /// lane never sees the despawn, only the view that no longer contains it,
    /// and deriving the answer means nothing has to be notified.
    pub fn retain_live(&mut self, alive: impl Fn(EntityId) -> bool) {
        self.instances.retain(|(entity, _), _| alive(*entity));
    }

    /// How many instances are held.
    pub fn instance_count(&self) -> usize {
        self.instances.len()
    }
}
