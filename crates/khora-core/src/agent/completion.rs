// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Per-frame agent completion tracking — how the scheduler records which
//! agents have finished the frame, and how.
//!
//! At frame start the scheduler builds an [`AgentCompletionMap`] with one
//! entry per known agent. After each agent's run (or skip), the
//! scheduler calls [`AgentCompletionMap::mark`] with a [`CompletionOutcome`];
//! dependents read it with [`AgentCompletionMap::outcome`].

use std::collections::HashMap;
use std::sync::Mutex;

use crate::agent::gorna::AgentId;

/// What happened to an agent this frame, from the scheduler's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionOutcome {
    /// The agent's run went to completion.
    Completed,
    /// The agent was skipped (budget pressure, missing dep, etc.).
    Skipped,
    /// The agent's run panicked. The scheduler caught it: the frame goes on, and
    /// whatever the agent wrote is dropped.
    Failed,
}

/// Frame-scoped map of agent outcomes.
///
/// Created by the scheduler at the top of each frame and passed by
/// reference to the frame's execution; nothing keeps it past the frame.
pub struct AgentCompletionMap {
    /// Each known agent's outcome, `None` until it is marked.
    entries: HashMap<AgentId, Mutex<Option<CompletionOutcome>>>,
}

impl AgentCompletionMap {
    /// Builds a completion map with one unmarked entry per agent ID.
    pub fn new(agent_ids: &[AgentId]) -> Self {
        let entries = agent_ids.iter().map(|id| (*id, Mutex::new(None))).collect();
        Self { entries }
    }

    /// Marks an agent as having finished this frame with the given outcome.
    ///
    /// The first outcome sticks, with one exception: [`Failed`] replaces an
    /// earlier one. An agent that runs several fixed sub-steps in a frame and
    /// panics on a later one has failed the frame, even though its first
    /// sub-step completed.
    /// Returns `false` if the agent ID is unknown to this map.
    ///
    /// [`Failed`]: CompletionOutcome::Failed
    pub fn mark(&self, id: AgentId, outcome: CompletionOutcome) -> bool {
        match self.entries.get(&id) {
            Some(entry) => {
                let mut held = entry.lock().unwrap_or_else(|e| e.into_inner());
                if held.is_none() || outcome == CompletionOutcome::Failed {
                    *held = Some(outcome);
                }
                true
            }
            None => false,
        }
    }

    /// Returns the recorded outcome for `id`, or `None` if the agent has
    /// not yet been marked (or the ID is unknown).
    pub fn outcome(&self, id: AgentId) -> Option<CompletionOutcome> {
        *self
            .entries
            .get(&id)?
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// Returns `true` if a [`mark`](Self::mark) has been recorded for `id`.
    pub fn is_done(&self, id: AgentId) -> bool {
        self.outcome(id).is_some()
    }

    /// Returns all agent IDs tracked by this map (for tests / introspection).
    pub fn known_ids(&self) -> impl Iterator<Item = AgentId> + '_ {
        self.entries.keys().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_outcome_is_done_once_marked() {
        let map = AgentCompletionMap::new(&[AgentId::new("renderer"), AgentId::new("shadows")]);
        assert!(!map.is_done(AgentId::new("renderer")));
        assert!(!map.is_done(AgentId::new("shadows")));

        map.mark(AgentId::new("renderer"), CompletionOutcome::Completed);
        assert!(map.is_done(AgentId::new("renderer")));
        assert!(!map.is_done(AgentId::new("shadows")));

        // A skipped agent is done too: `is_done` means an outcome was recorded.
        map.mark(AgentId::new("shadows"), CompletionOutcome::Skipped);
        assert!(map.is_done(AgentId::new("shadows")));

        // An id the map does not track is never done, even after a refused mark.
        assert!(!map.mark(AgentId::new("audio"), CompletionOutcome::Completed));
        assert!(!map.is_done(AgentId::new("audio")));
    }

    #[test]
    fn failed_replaces_an_earlier_outcome() {
        let map = AgentCompletionMap::new(&[AgentId::new("physics")]);
        map.mark(AgentId::new("physics"), CompletionOutcome::Completed);
        assert!(map.mark(AgentId::new("physics"), CompletionOutcome::Failed));
        assert_eq!(
            map.outcome(AgentId::new("physics")),
            Some(CompletionOutcome::Failed)
        );
        assert!(map.is_done(AgentId::new("physics")));

        // A later non-failure does not undo the failure.
        map.mark(AgentId::new("physics"), CompletionOutcome::Completed);
        assert_eq!(
            map.outcome(AgentId::new("physics")),
            Some(CompletionOutcome::Failed)
        );
    }

    #[test]
    fn outcome_distinguishes_completed_from_skipped() {
        let map = AgentCompletionMap::new(&[AgentId::new("renderer"), AgentId::new("shadows")]);
        map.mark(AgentId::new("renderer"), CompletionOutcome::Completed);
        map.mark(AgentId::new("shadows"), CompletionOutcome::Skipped);

        assert_eq!(
            map.outcome(AgentId::new("renderer")),
            Some(CompletionOutcome::Completed)
        );
        assert_eq!(
            map.outcome(AgentId::new("shadows")),
            Some(CompletionOutcome::Skipped)
        );
        assert_eq!(map.outcome(AgentId::new("physics")), None);
    }

    #[test]
    fn mark_is_idempotent() {
        let map = AgentCompletionMap::new(&[AgentId::new("renderer")]);
        assert!(map.mark(AgentId::new("renderer"), CompletionOutcome::Completed));
        // Second mark is accepted (returns true: agent is known) but the
        // outcome stays at the first value.
        assert!(map.mark(AgentId::new("renderer"), CompletionOutcome::Skipped));
        assert_eq!(
            map.outcome(AgentId::new("renderer")),
            Some(CompletionOutcome::Completed)
        );
    }

    #[test]
    fn mark_unknown_agent_returns_false() {
        let map = AgentCompletionMap::new(&[AgentId::new("renderer")]);
        assert!(!map.mark(AgentId::new("audio"), CompletionOutcome::Completed));
    }
}
