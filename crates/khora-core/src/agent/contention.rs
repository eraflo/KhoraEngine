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

//! What an agent competes with the others for.
//!
//! An agent used to declare one thing — the deck slots it wrote — and the
//! scheduler checked it. It declared **nothing** about what it read or locked in
//! the [`Runtime`](crate::Runtime), so the scheduler had no way to know, and
//! [`AgentAccess`](super::AgentAccess) had to stand in for the answer by
//! forbidding every resource to any agent that wanted to run concurrently.
//!
//! That is why scripting grew a route of its own: its agent is world-free, so it
//! claimed `Isolated`, so it could not read the queue its own hot-reload pump
//! filled — and a pair of "flows" that projected nothing appeared to carry the
//! queue in through the one door an `Isolated` agent could use.
//!
//! It bites elsewhere too. `InputMap` is published every frame and **nothing in
//! the CLAD descent reads it**. And the render and UI agents, both `SharedWorld`
//! and both in `OUTPUT`, cannot share a wave — not because they contend, but
//! because nobody could say whether they did.
//!
//! # Reading is free, locking is not
//!
//! [`Resources::get`](crate::runtime::Resources::get) hands out `&T`, so a
//! resource is mutated through the inside — `Arc<Mutex<InputMap>>`. "Writing"
//! therefore means **taking the lock**, which is exactly the contention that
//! matters: two agents running at once on the same mutex serialise, and the
//! scheduler has to know that before it groups them, not measure it afterwards.

use std::any::TypeId;

/// What one agent touches that another might.
///
/// Three surfaces in one declaration, so an agent's contention reads in one
/// place. `deck` was once its own method and is folded in here: it was the only
/// half that existed, and keeping it apart would have suggested the two answer
/// different questions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Contention {
    /// [`OutputDeck`](crate::lane::OutputDeck) slot types the agent writes.
    ///
    /// Co-wave agents write into private shards folded back together
    /// afterwards, so a slot written by two of them would lose one of the two at
    /// the merge.
    pub deck: Vec<TypeId>,

    /// Runtime resources and services the agent reads without locking.
    ///
    /// Any number of agents may read the same one at once — that is the whole
    /// point of saying so.
    pub reads: Vec<TypeId>,

    /// Runtime resources and services the agent **locks**, to mutate or drain.
    pub writes: Vec<TypeId>,
}

impl Contention {
    /// An agent that contends for nothing.
    pub fn none() -> Self {
        Self::default()
    }

    /// Declares the deck slots written.
    pub fn writing_deck(mut self, slots: impl IntoIterator<Item = TypeId>) -> Self {
        self.deck.extend(slots);
        self
    }

    /// Declares a resource read without locking.
    pub fn reading<T: 'static>(mut self) -> Self {
        self.reads.push(TypeId::of::<T>());
        self
    }

    /// Declares a resource locked to mutate or drain.
    pub fn locking<T: 'static>(mut self) -> Self {
        self.writes.push(TypeId::of::<T>());
        self
    }

    /// Whether the agent declared reaching `T` at all.
    pub fn may_reach(&self, ty: TypeId) -> bool {
        self.reads.contains(&ty) || self.writes.contains(&ty)
    }

    /// Whether the agent declared **locking** `T`.
    pub fn may_lock(&self, ty: TypeId) -> bool {
        self.writes.contains(&ty)
    }

    /// What two agents in the same wave would fight over.
    ///
    /// Reads do not conflict with reads. Everything else does: two writers
    /// serialise on the lock, and a reader beside a writer sees a half-written
    /// state.
    pub fn conflicts_with(&self, other: &Self) -> Vec<TypeId> {
        let mut found: Vec<TypeId> = self
            .deck
            .iter()
            .filter(|ty| other.deck.contains(ty))
            .chain(
                self.writes
                    .iter()
                    .filter(|ty| other.writes.contains(ty) || other.reads.contains(ty)),
            )
            .chain(self.reads.iter().filter(|ty| other.writes.contains(ty)))
            .copied()
            .collect();
        found.dedup();
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Input;
    struct Time;
    struct Draws;

    #[test]
    fn an_agent_declares_nothing_by_default() {
        let quiet = Contention::none();

        assert!(!quiet.may_reach(TypeId::of::<Input>()));
        assert!(!quiet.may_lock(TypeId::of::<Input>()));
    }

    /// Locking implies reaching: an agent that declared a lock does not also
    /// have to declare a read to look at what it locked.
    #[test]
    fn locking_a_resource_also_permits_reaching_it() {
        let draining = Contention::none().locking::<Input>();

        assert!(draining.may_reach(TypeId::of::<Input>()));
        assert!(draining.may_lock(TypeId::of::<Input>()));
    }

    /// But reading does not imply locking — that is the distinction the whole
    /// contract turns on.
    #[test]
    fn reading_a_resource_does_not_permit_locking_it() {
        let looking = Contention::none().reading::<Input>();

        assert!(looking.may_reach(TypeId::of::<Input>()));
        assert!(!looking.may_lock(TypeId::of::<Input>()));
    }

    // ─── What conflicts ─────────────────────────────────────────────────────

    /// **The gain.** Two readers of the same resource are not a conflict, which
    /// is what lets several agents share `InputMap` in one wave.
    #[test]
    fn two_readers_do_not_conflict() {
        let one = Contention::none().reading::<Input>();
        let other = Contention::none().reading::<Input>();

        assert!(one.conflicts_with(&other).is_empty());
    }

    #[test]
    fn two_writers_conflict() {
        let one = Contention::none().locking::<Input>();
        let other = Contention::none().locking::<Input>();

        assert_eq!(one.conflicts_with(&other), vec![TypeId::of::<Input>()]);
    }

    /// A reader beside a writer sees a half-written state, so it counts —
    /// whichever side is asked.
    #[test]
    fn a_reader_and_a_writer_conflict_both_ways() {
        let reader = Contention::none().reading::<Input>();
        let writer = Contention::none().locking::<Input>();

        assert_eq!(reader.conflicts_with(&writer), vec![TypeId::of::<Input>()]);
        assert_eq!(writer.conflicts_with(&reader), vec![TypeId::of::<Input>()]);
    }

    #[test]
    fn a_shared_deck_slot_conflicts() {
        let one = Contention::none().writing_deck([TypeId::of::<Draws>()]);
        let other = Contention::none().writing_deck([TypeId::of::<Draws>()]);

        assert_eq!(one.conflicts_with(&other), vec![TypeId::of::<Draws>()]);
    }

    /// Different resources are not a conflict, however many each agent names.
    #[test]
    fn distinct_surfaces_do_not_conflict() {
        let one = Contention::none()
            .reading::<Input>()
            .locking::<Time>()
            .writing_deck([TypeId::of::<Draws>()]);
        let other = Contention::none().reading::<Time>().reading::<Input>();

        assert_eq!(
            one.conflicts_with(&other),
            vec![TypeId::of::<Time>()],
            "only the one the first locks and the second reads"
        );
    }
}
