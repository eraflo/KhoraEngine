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

//! The world's side of [`EntityReserver`]: ids a script takes in its turn,
//! given slots, committed by the spawn that uses them, the rest taken back —
//! all at the frame's exclusive moment.

use std::sync::Arc;

use khora_core::ecs::entity::EntityId;
use khora_core::ecs::EntityReserver;

use crate::ecs::ComponentBundle;

use super::World;

/// The fewest recycled ids the reserver keeps ready between frames.
const READY: usize = 16;

impl World {
    /// This world's reserver: ids a world-free caller takes through a shared
    /// reference, committed or released by this world at the frame boundary.
    pub fn reserver(&self) -> Arc<EntityReserver> {
        Arc::clone(&self.reserver)
    }

    /// A hold over this world's reserver: ids a load prepared off the frame
    /// reserves through it are not released at a frame's end, until the load
    /// commits them, or is abandoned or dropped.
    pub fn hold_reservations(&self) -> khora_core::ecs::ReservationHold {
        self.reserver.hold()
    }

    /// A hold over this world's reserver that holds the slots despawns freed:
    /// it hands those out first, then ids as [`hold_reservations`]'s does.
    /// The slots are held — no frame's end takes them back, no spawn is
    /// handed one — until the load commits them; dropping the hold gives
    /// back every one it did not commit.
    ///
    /// [`hold_reservations`]: Self::hold_reservations
    pub fn hold_freed(&mut self) -> khora_core::ecs::ReservationHold {
        self.settle_reservations();
        let freed: Vec<EntityId> =
            std::iter::from_fn(|| self.entities.reserve_recycled()).collect();
        self.reserver.hold_these(freed)
    }

    /// Makes at least `wanted` ids ready for the reserver to hand out first,
    /// from slots despawns freed — none past the end of the store — so the
    /// reservations a load is about to make reuse the store.
    pub fn ready_reservations(&mut self, wanted: usize) {
        self.settle_reservations();
        let missing = wanted.saturating_sub(self.reserver.ready_len());
        let recycled: Vec<EntityId> = (0..missing)
            .map_while(|_| self.entities.reserve_recycled())
            .collect();
        self.reserver.refill(recycled);
    }

    /// Brings `entity`, an id `hold` reserved, to life as a load commits it:
    /// `false`, changing nothing, for an id the hold does not hold, or one
    /// another world's reserver gave.
    pub(crate) fn commit_held(
        &mut self,
        hold: &khora_core::ecs::ReservationHold,
        entity: EntityId,
        empty: bool,
    ) -> bool {
        if !hold.is_over(&self.reserver) {
            return false;
        }
        self.settle_reservations();
        if !hold.commit(entity) {
            return false;
        }
        self.bring_reserved_to_life(entity, empty)
    }

    /// Turns the ids the reserver handed out past the end of the store into
    /// reserved slots, so a spawn can commit them.
    pub fn settle_reservations(&mut self) {
        let end = self.entities.slots() as u32;
        for index in self.reserver.fresh_since(end) {
            self.entities.hold_fresh(EntityId {
                index,
                generation: 0,
            });
        }
    }

    /// Releases every id the reserver handed out and nothing committed, then
    /// refills the ids it hands out next.
    ///
    /// As many are kept ready as the frame took, never fewer than [`READY`]:
    /// a game spawning and despawning a steady number a frame is handed back
    /// what it despawned rather than indices past the end of the store, and
    /// what a burst held back is freed once the rate falls.
    pub fn release_unspent(&mut self) {
        self.settle_reservations();
        for id in self.reserver.take_unspent() {
            self.entities.release_reserved(id);
        }
        let wanted = self.reserver.take_served().max(READY);
        for id in self.reserver.take_ready_beyond(wanted) {
            self.entities.release_reserved(id);
        }
        let missing = wanted.saturating_sub(self.reserver.ready_len());
        let recycled: Vec<EntityId> = (0..missing)
            .map_while(|_| self.entities.reserve_recycled())
            .collect();
        self.reserver.refill(recycled);
    }

    /// Spawns `bundle` at `entity`, an id this world's reserver handed out —
    /// `false`, changing nothing, for any other id: one another world's
    /// reserver gave, one already committed, or one released.
    pub fn spawn_at<B: ComponentBundle>(&mut self, entity: EntityId, bundle: B) -> bool {
        if B::unregistered(&self.storage.registry).is_some() || B::repeated().is_some() {
            return false;
        }
        self.settle_reservations();
        if self.nowhere_entity() == Some(entity) || !self.reserver.take_handed(entity) {
            return false;
        }
        if !self.entities.commit_reserved(entity) {
            return false;
        }
        self.place(entity, bundle);
        true
    }

    /// Keeps the reserver's fresh indices past every slot the store holds —
    /// after anything that may have added one.
    pub(super) fn after_allocation(&mut self) {
        let _ = self.reserver.fresh_since(self.entities.slots() as u32);
    }
}
