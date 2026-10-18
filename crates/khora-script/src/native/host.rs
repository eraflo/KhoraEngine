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

//! The host side of a script call: the world view and the queues a native
//! writes into.

use khora_core::ecs::entity::EntityId;
use khora_core::script::{CommandBuffer, EventQueue};

use super::call_context::NativeContext;
use super::function_registry::NativeRegistry;
use crate::arena::{Arena, PersistentStore};

/// Everything a running program needs from outside itself.
///
/// Owns what [`NativeContext`] only borrows, so a caller holds one of these for
/// a frame rather than assembling the pieces at every [`run`] — and so the
/// commands a script queued are still there afterwards to be applied.
///
/// [`run`]: crate::vm::Machine::run
#[derive(Debug)]
pub struct Host {
    /// What the program may call.
    pub natives: NativeRegistry,
    /// Effects queued for the frame boundary.
    pub commands: CommandBuffer,
    /// Frame memory.
    pub arena: Arena,
    /// The running behavior's fields.
    ///
    /// Swapped in per behavior instance rather than owned by the machine: the
    /// machine is the *call*, and a field outlives every call made on the
    /// entity. This is also what a scene save writes out, which is why it is a
    /// [`PersistentStore`] and not more registers.
    pub fields: PersistentStore,
    /// What the player is holding, for the whole frame.
    ///
    /// Set once by the lane from the view, not per behavior: input is a fact
    /// about the frame rather than about an entity, and every behavior in it
    /// sees the same one. A behavior that read it halfway through a frame in
    /// which another behavior had "changed" it would be reading something no
    /// player did.
    pub input: khora_core::platform::InputSnapshot,
    /// How long the running program asked to wait, set by `await`.
    ///
    /// Cleared by whoever acts on it. Left here rather than returned from
    /// `run`, because a suspension for fuel and a suspension for time are the
    /// same stop — what differs is only what the caller should do next.
    pub awaiting: Option<f32>,
    /// The entity the running behavior belongs to.
    pub entity: Option<EntityId>,
    /// Where that entity is, from the frame's view.
    ///
    /// Swapped in per behavior alongside [`entity`](Self::entity): they name the
    /// same subject, and a position left over from the previous behavior would
    /// be worse than none at all.
    pub position: Option<khora_core::math::Vec3>,
    /// What this frame's scripts raised, for the next frame to deliver.
    ///
    /// The frame's, like [`commands`](Self::commands) — one behavior can raise
    /// an event another will hear, and both are collected here until whoever
    /// owns the frame takes them.
    pub outbox: EventQueue,
    /// The components scripts may read this frame, and what this frame's
    /// scripts wrote over them.
    ///
    /// Set once by the lane from the view, for the frame: every behavior reads
    /// the same working set, and each one's writes are in it for the next.
    pub components: khora_core::script::FrameComponents,
    /// How many rows this frame's queries took: what their sweeps cost the
    /// frame, for the budget's next estimate.
    pub swept: usize,
    /// Where a `Spawn` takes its entity's id: the reserver of the world its
    /// commands are for, or one of its own for a program run without a world.
    pub reserver: std::sync::Arc<khora_core::ecs::EntityReserver>,
    /// The prefabs the project holds, when the host knows them: a prefab
    /// `Spawn` naming another faults its turn. `None` accepts any.
    pub prefabs: Option<std::sync::Arc<std::collections::BTreeSet<khora_core::asset::AssetUUID>>>,
}

impl Default for Host {
    fn default() -> Self {
        Self::new()
    }
}

impl Host {
    /// A host exposing everything the engine registered.
    ///
    /// [`NativeRegistry::discovered`] rather than just the built-ins, and it has
    /// to match what the program was compiled against — which is why
    /// [`compile`](crate::compile) and [`check`](crate::check) take the same
    /// one.
    pub fn new() -> Self {
        Self {
            natives: NativeRegistry::discovered(),
            commands: CommandBuffer::new(),
            arena: Arena::new(),
            fields: PersistentStore::new(),
            awaiting: None,
            entity: None,
            position: None,
            input: Default::default(),
            outbox: EventQueue::new(),
            components: Default::default(),
            swept: 0,
            reserver: std::sync::Arc::new(khora_core::ecs::EntityReserver::new()),
            prefabs: None,
        }
    }

    /// Names the behavior instance whose fields are in play.
    ///
    /// Swapping the store rather than the host is what lets one host serve
    /// every behavior in a frame: the arena, the command buffer and the
    /// registry are the frame's, while the fields belong to one entity.
    pub fn with_fields(mut self, fields: PersistentStore) -> Self {
        self.fields = fields;
        self
    }

    /// Lends the host the frame memory to run in.
    ///
    /// The arena outlives any one host: whoever runs frame after frame keeps
    /// one and lends it each frame, so its generation keeps counting and a
    /// reference from an earlier frame is always stale — never a read of
    /// whatever the next frame put at its index, which an arena made new each
    /// frame, starting its count over, would allow.
    pub fn with_arena(mut self, arena: Arena) -> Self {
        self.arena = arena;
        self
    }

    /// Hands the host the reserver of the world its commands are for: a
    /// `Spawn` takes its entity's id from it.
    pub fn with_reserver(
        mut self,
        reserver: std::sync::Arc<khora_core::ecs::EntityReserver>,
    ) -> Self {
        self.reserver = reserver;
        self
    }

    /// Hands the host the prefabs the project holds: a prefab `Spawn` naming
    /// one it does not hold faults the turn. A host given none accepts any.
    pub fn with_prefabs(
        mut self,
        prefabs: std::sync::Arc<std::collections::BTreeSet<khora_core::asset::AssetUUID>>,
    ) -> Self {
        self.prefabs = Some(prefabs);
        self
    }

    /// Gives the lent arena back, once the frame has ended.
    pub fn take_arena(&mut self) -> Arena {
        std::mem::take(&mut self.arena)
    }

    /// A host exposing nothing at all, for a program that must call nothing.
    pub fn bare() -> Self {
        Self {
            natives: NativeRegistry::new(),
            ..Self::new()
        }
    }

    /// Names the entity the running behavior belongs to.
    pub fn for_entity(mut self, entity: EntityId) -> Self {
        self.entity = Some(entity);
        self
    }

    /// Ends the frame: the arena is freed and the queued commands handed over.
    ///
    /// Both in one step because they belong to the same moment — releasing the
    /// arena while the commands still referred to it is exactly the mistake the
    /// generation counter exists to catch, and doing it here means no caller has
    /// to remember the order.
    pub fn end_frame(&mut self) -> CommandBuffer {
        self.arena.reset();
        std::mem::take(&mut self.commands)
    }

    /// Takes the events this frame raised, for the next one to deliver.
    ///
    /// Separate from [`end_frame`](Self::end_frame) because they go to different
    /// places: commands leave for the `World`, events come back to scripting.
    pub fn take_events(&mut self) -> EventQueue {
        std::mem::take(&mut self.outbox)
    }

    /// The narrow view a native gets.
    ///
    /// `strings` comes from the program rather than the host: the same host
    /// runs whatever program the frame hands it, and a string constant belongs
    /// to the program it was compiled into.
    pub fn context<'a>(&'a mut self, strings: &'a [String]) -> NativeContext<'a> {
        NativeContext {
            commands: &mut self.commands,
            arena: &mut self.arena,
            entity: self.entity,
            strings,
            position: self.position,
            input: &self.input,
            events: &mut self.outbox,
        }
    }
}
