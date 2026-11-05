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

//! Which instances have left: an entity despawned, or a behavior detached
//! from an entity that lives on — and forgetting them.

use khora_core::ecs::entity::EntityId;
use khora_script::native::Host;

use super::frame::FUEL_PER_BEHAVIOR;
use super::hooks::say_goodbye;
use super::report::ScriptRunReport;
use super::runtime::ScriptRuntime;

impl ScriptRuntime {
    /// Forgets the instances `left` names, by entity and behavior, without a
    /// farewell: they left the family's scope while still attached.
    pub(super) fn forget(&mut self, left: &[(EntityId, String)]) {
        if left.is_empty() {
            return;
        }
        self.retain_behaviors(|entity, behavior| {
            !left
                .iter()
                .any(|(gone, name)| *gone == entity && name == behavior)
        });
    }

    /// Instances whose entity the predicate no longer keeps, and which have not
    /// yet said goodbye.
    ///
    /// Returned as `(entity, behavior, module)` rather than acted on here: the
    /// farewell is a *run*, which needs a host and a budget, and neither belongs
    /// to a store of state. The caller runs them and then calls
    /// [`retain_live`](Self::retain_live) to drop them.
    pub fn departed(&self, alive: impl Fn(EntityId) -> bool) -> Vec<(EntityId, String, String)> {
        self.departed_behaviors(|entity, _| alive(entity))
    }

    /// [`departed`](Self::departed), by instance: an entity still alive whose
    /// behavior was detached — its component removed — has left too.
    pub fn departed_behaviors(
        &self,
        alive: impl Fn(EntityId, &str) -> bool,
    ) -> Vec<(EntityId, String, String)> {
        let mut departed = self
            .instances
            .iter()
            // Only an instance that announced itself says goodbye: one that
            // never had a turn — its arrival still held — never began. And
            // not a disabled one: a behavior that faulted is not called again.
            .filter(|((entity, behavior), instance)| {
                instance.spawned
                    && !instance.disabled
                    && !instance.farewelled
                    && !alive(*entity, behavior)
            })
            .map(|((entity, behavior), instance)| {
                (*entity, behavior.clone(), instance.module.clone())
            })
            .collect::<Vec<_>>();
        // In entity order and, on one entity, in the order its behaviors take
        // their turns — by module, then name — whatever the map's: farewells
        // are script work, and their order is observable.
        departed.sort_by(|a, b| {
            (a.0.index, a.0.generation, &a.2, &a.1).cmp(&(b.0.index, b.0.generation, &b.2, &b.1))
        });
        departed
    }

    /// Forgets every instance the given predicate does not keep.
    ///
    /// Called with the frame's live entities, which is how a despawned one's
    /// fields are released. Sweeping rather than reacting to a despawn: the
    /// lane never sees the despawn, only the view that no longer contains it,
    /// and deriving the answer means nothing has to be notified.
    pub fn retain_live(&mut self, alive: impl Fn(EntityId) -> bool) {
        self.retain_behaviors(|entity, _| alive(entity));
    }

    /// [`retain_live`](Self::retain_live), by instance: a behavior detached
    /// from an entity that lives on is forgotten too.
    pub fn retain_behaviors(&mut self, alive: impl Fn(EntityId, &str) -> bool) {
        self.instances
            .retain(|(entity, behavior), _| alive(*entity, behavior));
        self.attached.retain(alive);
    }
}

/// Runs `OnDespawn` for every instance whose entity has left the view.
///
/// The other half of the farewell: a script that despawns itself is told at the
/// moment it decides, and everything else — an editor deletion, another system's
/// despawn — is noticed here, one frame later. That lateness is not an oversight
/// but the earliest a lane reading a projection can know, and it is why the
/// script-initiated path exists at all.
pub(super) fn farewell_departed(
    live: &impl Fn(khora_core::ecs::entity::EntityId, &str) -> bool,
    positions: &std::collections::HashMap<
        khora_core::ecs::entity::EntityId,
        khora_core::math::Vec3,
    >,
    runtime: &mut ScriptRuntime,
    host: &mut Host,
    fuel: u64,
    report: &mut ScriptRunReport,
) {
    for (entity, behavior, module) in runtime.departed_behaviors(live) {
        let left = fuel.saturating_sub(report.spent);
        if left == 0 {
            return;
        }
        let Some(program) = runtime.program(&module).cloned() else {
            continue;
        };

        host.entity = Some(entity);
        // An entity that has left the view has no pose to give: `Position()`
        // faults rather than answering with where it used to be — the other
        // half of why a script that despawns itself is told at the moment it
        // decides. A behavior detached from an entity still in the view reads
        // where it is.
        host.position = positions.get(&entity).copied();
        host.fields = std::mem::take(&mut runtime.instance(entity, &behavior).fields);
        let ran = std::time::Instant::now();
        report.spent += say_goodbye(&program, &behavior, host, left.min(FUEL_PER_BEHAVIOR));
        report.turns_time += ran.elapsed();
        host.fields = khora_script::arena::PersistentStore::new();
    }
}
