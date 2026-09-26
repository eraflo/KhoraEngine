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

//! Grouping a phase's agents into concurrent waves, and checking that the
//! agents sharing a wave do not contend.

use super::AgentSlot;
use khora_core::agent::dependency::DependencyKind;
use khora_core::agent::AgentAccess;
use khora_core::agent::Contention;
use khora_core::control::gorna::AgentId;

/// Per-agent metadata the parallel executor needs to group agents into waves.
pub(super) struct WaveMeta {
    /// The agent's id, or `None` if it could not be locked (→ singleton, skipped).
    pub(super) id: Option<AgentId>,
    /// Whether the agent is safe to run concurrently.
    pub(super) access: AgentAccess,
    /// Ids this agent hard-depends on (must run in an earlier wave).
    pub(super) hard_dep_targets: Vec<AgentId>,
    /// What this agent competes for (from [`Agent::contention`]), used to verify
    /// co-wave agents do not collide before dispatch, and stamped onto the
    /// context so an undeclared reach is refused at the point of access.
    pub(super) contention: Contention,
}

/// Reads each agent's id, access footprint, and declared contention once,
/// building the [`WaveMeta`] the wave partitioner + disjointness check need. A
/// failed lock yields `id: None` → an always-singleton, always-skipped slot.
///
/// Once, and reused: the context is stamped with the same `Contention` the wave
/// was built from, so what the scheduler checked and what the agent is held to
/// cannot be two different answers.
pub(super) fn build_wave_metas(agents: &[AgentSlot]) -> Vec<WaveMeta> {
    agents
        .iter()
        .map(|(agent, _, _, deps)| {
            let (id, access, contention) = agent
                .lock()
                .ok()
                .map(|a| (Some(a.id()), a.access(), a.contention()))
                .unwrap_or((None, AgentAccess::Exclusive, Contention::none()));
            WaveMeta {
                id,
                access,
                hard_dep_targets: deps
                    .iter()
                    .filter(|d| matches!(d.kind, DependencyKind::Hard))
                    .map(|d| d.target)
                    .collect(),
                contention,
            }
        })
        .collect()
}

/// Groups agents (already in `sort_agents` order) into execution waves.
///
/// A wave is a maximal run of consecutive concurrency-eligible agents
/// ([`AgentAccess::Isolated`] or [`AgentAccess::SharedWorld`]) in which no
/// member hard-depends on another member and no member's
/// [`Contention`](khora_core::agent::Contention) overlaps another's. Any
/// [`AgentAccess::Exclusive`] agent is its own singleton wave. Because the input
/// is already topologically ordered by hard deps, and a dependent never shares
/// a wave with its target, running the waves in order preserves the exact
/// Hard-dependency ordering the sequential path guarantees.
pub(super) fn partition_waves(metas: &[WaveMeta]) -> Vec<Vec<usize>> {
    let mut waves: Vec<Vec<usize>> = Vec::new();
    for (i, meta) in metas.iter().enumerate() {
        let joins_current = meta.access != AgentAccess::Exclusive
            && waves.last().is_some_and(|wave| {
                // every current member is concurrency-eligible …
                wave.iter().all(|&j| metas[j].access != AgentAccess::Exclusive)
                    // … no member is this agent's hard-dep target …
                    && !meta.hard_dep_targets.iter().any(|target| {
                        wave.iter().any(|&j| metas[j].id == Some(*target))
                    })
                    // … and nothing it declared collides with a member's.
                    //
                    // This once refused a second `SharedWorld` outright, because
                    // such an agent "may write shared resources" and nobody
                    // could say which. Now they say, so `RenderAgent` and
                    // `UiAgent` may share a wave when their declarations are
                    // disjoint — parallelism gained by removing a constraint.
                    && wave.iter().all(|&j| {
                        metas[j].contention.conflicts_with(&meta.contention).is_empty()
                    })
            });
        if joins_current {
            waves.last_mut().expect("checked non-empty").push(i);
        } else {
            waves.push(vec![i]);
        }
    }
    waves
}

/// Logs an error for every surface two agents in the same concurrent `wave`
/// would fight over.
///
/// A deck slot written twice loses one of the two when the private shards are
/// folded back; a resource locked twice serialises the pair the wave existed to
/// run at once; a resource locked by one and read by another shows the reader a
/// half-written state. Reading the same thing is not a conflict, which is the
/// whole point of declaring it.
///
/// [`Agent::contention`](khora_core::agent::Agent::contention) is what makes
/// this checkable at wave-formation time — naming the offending agents — rather
/// than discovered defensively at the merge, or not at all.
///
/// Returns the number of collisions detected (0 = disjoint, the expected case).
pub(super) fn check_wave_disjoint(wave: &[usize], metas: &[WaveMeta]) -> usize {
    let mut collisions = 0;
    for (position, &idx) in wave.iter().enumerate() {
        let Some(id) = metas[idx].id else { continue };
        for &other in &wave[position + 1..] {
            let Some(prev) = metas[other].id else {
                continue;
            };
            for slot in metas[idx]
                .contention
                .conflicts_with(&metas[other].contention)
            {
                collisions += 1;
                log::error!(
                    "Scheduler: agents {prev:?} and {id:?} share a concurrent wave but both write \
                     OutputDeck slot {slot:?} — their shards will collide on merge \
                     (parallel-eligibility bug)"
                );
            }
        }
    }
    collisions
}
