// Copyright 2025 eraflo
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0

//! Per-frame agent completion tracking — the synchronization primitive used
//! by the scheduler to coordinate agents that declare `Hard` dependencies.
//!
//! At frame start the scheduler builds an [`AgentCompletionMap`] populated
//! with one [`StageHandle<AgentDone>`] per known agent. After each agent
//! `execute()` (or skip), the scheduler calls [`AgentCompletionMap::mark`]
//! with a [`CompletionOutcome`].  Dependents either inspect the outcome
//! synchronously via [`AgentCompletionMap::outcome`] or — once the parallel
//! scheduler is enabled — `await` it via [`AgentCompletionMap::wait`].

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::control::gorna::AgentId;
use crate::renderer::api::core::StageHandle;

/// Marker type for the "an agent finished its frame work" stage.
pub struct AgentDone;

/// What happened to an agent this frame, from the scheduler's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionOutcome {
    /// `execute()` ran to completion.
    Completed,
    /// The agent was skipped (budget pressure, missing dep, etc.).
    Skipped,
}

/// One entry in the completion map: a level-triggered handle plus a
/// once-set outcome flag.
struct AgentCompletion {
    handle: StageHandle<AgentDone>,
    outcome: OnceLock<CompletionOutcome>,
}

impl AgentCompletion {
    fn new() -> Self {
        Self {
            handle: StageHandle::<AgentDone>::default(),
            outcome: OnceLock::new(),
        }
    }
}

/// Frame-scoped map of agent completion handles.
///
/// Created by the scheduler at the top of each frame and inserted into
/// the per-frame [`ServiceRegistry`](crate::ServiceRegistry) overlay.
/// Agents must NOT retain a reference past the current frame.
pub struct AgentCompletionMap {
    entries: HashMap<AgentId, AgentCompletion>,
}

impl AgentCompletionMap {
    /// Builds a completion map with one empty handle per agent ID.
    pub fn new(agent_ids: &[AgentId]) -> Self {
        let mut entries = HashMap::with_capacity(agent_ids.len());
        for id in agent_ids {
            entries.insert(*id, AgentCompletion::new());
        }
        Self { entries }
    }

    /// Marks an agent as having finished this frame with the given outcome.
    ///
    /// Idempotent: subsequent calls for the same agent are no-ops.
    /// Returns `false` if the agent ID is unknown to this map.
    pub fn mark(&self, id: AgentId, outcome: CompletionOutcome) -> bool {
        match self.entries.get(&id) {
            Some(entry) => {
                let _ = entry.outcome.set(outcome);
                entry.handle.mark_done();
                true
            }
            None => false,
        }
    }

    /// Returns the recorded outcome for `id`, or `None` if the agent has
    /// not yet been marked (or the ID is unknown).
    pub fn outcome(&self, id: AgentId) -> Option<CompletionOutcome> {
        self.entries.get(&id)?.outcome.get().copied()
    }

    /// Returns `true` if a [`mark`](Self::mark) has been recorded for `id`.
    pub fn is_done(&self, id: AgentId) -> bool {
        self.entries
            .get(&id)
            .is_some_and(|entry| entry.handle.is_done())
    }

    /// Awaits an agent's completion. Returns the outcome, or `None` if the
    /// agent ID is unknown to this map (signals a configuration bug rather
    /// than blocking forever).
    pub async fn wait(&self, id: AgentId) -> Option<CompletionOutcome> {
        let entry = self.entries.get(&id)?;
        entry.handle.wait().await;
        entry.outcome.get().copied()
    }

    /// Returns all agent IDs tracked by this map (for tests / introspection).
    pub fn known_ids(&self) -> impl Iterator<Item = AgentId> + '_ {
        self.entries.keys().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn wait_returns_immediately_after_mark() {
        let map = AgentCompletionMap::new(&[AgentId::Renderer]);
        map.mark(AgentId::Renderer, CompletionOutcome::Completed);

        let result = rt().block_on(async { map.wait(AgentId::Renderer).await });
        assert_eq!(result, Some(CompletionOutcome::Completed));
    }

    #[test]
    fn wait_unblocks_when_mark_arrives_later() {
        use std::sync::Arc;

        let map = Arc::new(AgentCompletionMap::new(&[AgentId::ShadowRenderer]));
        let map_clone = Arc::clone(&map);

        let result = rt().block_on(async move {
            let waiter = tokio::spawn(async move { map_clone.wait(AgentId::ShadowRenderer).await });
            // Yield so the waiter has a chance to register.
            tokio::task::yield_now().await;
            map.mark(AgentId::ShadowRenderer, CompletionOutcome::Skipped);
            waiter.await.unwrap()
        });
        assert_eq!(result, Some(CompletionOutcome::Skipped));
    }

    #[test]
    fn wait_on_unknown_agent_returns_none() {
        let map = AgentCompletionMap::new(&[AgentId::Renderer]);
        let result = rt().block_on(async { map.wait(AgentId::Audio).await });
        assert_eq!(result, None);
    }

    #[test]
    fn outcome_distinguishes_completed_from_skipped() {
        let map = AgentCompletionMap::new(&[AgentId::Renderer, AgentId::ShadowRenderer]);
        map.mark(AgentId::Renderer, CompletionOutcome::Completed);
        map.mark(AgentId::ShadowRenderer, CompletionOutcome::Skipped);

        assert_eq!(
            map.outcome(AgentId::Renderer),
            Some(CompletionOutcome::Completed)
        );
        assert_eq!(
            map.outcome(AgentId::ShadowRenderer),
            Some(CompletionOutcome::Skipped)
        );
        assert_eq!(map.outcome(AgentId::Physics), None);
    }

    #[test]
    fn mark_is_idempotent() {
        let map = AgentCompletionMap::new(&[AgentId::Renderer]);
        assert!(map.mark(AgentId::Renderer, CompletionOutcome::Completed));
        // Second mark is accepted (returns true: agent is known) but the
        // outcome stays at the first value.
        assert!(map.mark(AgentId::Renderer, CompletionOutcome::Skipped));
        assert_eq!(
            map.outcome(AgentId::Renderer),
            Some(CompletionOutcome::Completed)
        );
    }

    #[test]
    fn mark_unknown_agent_returns_false() {
        let map = AgentCompletionMap::new(&[AgentId::Renderer]);
        assert!(!map.mark(AgentId::Audio, CompletionOutcome::Completed));
    }
}
