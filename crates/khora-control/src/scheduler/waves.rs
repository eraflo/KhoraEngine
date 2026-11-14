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
use khora_core::agent::gorna::AgentId;
use khora_core::agent::Contention;

/// What the executor needs of an agent to group it into a wave.
pub(super) struct WaveMeta {
    pub(super) id: AgentId,
    /// Ids this agent hard-depends on (must run in an earlier wave).
    pub(super) hard_dep_targets: Vec<AgentId>,
    /// What this agent competes for: checked against its wave-mates, and stamped onto its
    /// context so an undeclared reach is refused.
    pub(super) contention: Contention,
}

/// Reads each agent's id and its installed layer's contention once, as [`WaveMeta`]s.
///
/// The context is stamped with the same `Contention` the wave was built from, so what the
/// scheduler checked and what the agent is held to cannot differ.
pub(super) fn build_wave_metas(agents: &[AgentSlot]) -> Vec<WaveMeta> {
    agents
        .iter()
        .map(|(_, _, _, deps, known)| WaveMeta {
            id: known.id,
            hard_dep_targets: deps
                .iter()
                .filter(|d| matches!(d.kind, DependencyKind::Hard))
                .map(|d| d.target)
                .collect(),
            contention: known.contention(),
        })
        .collect()
}

/// Groups agents, in `sort_agents` order, into waves; with `concurrent` off, one per agent.
///
/// A wave is a maximal run of consecutive agents none of which hard-depends on another or
/// overlaps another's [`Contention`]. A dependent never
/// shares a wave with its target, so running the waves in order keeps Hard dependencies.
pub(super) fn partition_waves(metas: &[WaveMeta], concurrent: bool) -> Vec<Vec<usize>> {
    let mut waves: Vec<Vec<usize>> = Vec::new();
    for (i, meta) in metas.iter().enumerate() {
        let joins_current = concurrent
            && waves.last().is_some_and(|wave| {
                // no member is this agent's hard-dep target …
                !meta.hard_dep_targets.iter().any(|target| {
                    wave.iter().any(|&j| metas[j].id == *target)
                })
                    // … and nothing it declared collides with a member's.
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

/// Logs an error for every surface two agents of a concurrent `wave` would fight over,
/// returning their count.
///
/// A deck slot written twice loses one write at the fold; a resource locked twice
/// serialises the pair; a lock beside a read shows the reader a half-written state. Two
/// reads are no conflict.
pub(super) fn check_wave_disjoint(wave: &[usize], metas: &[WaveMeta]) -> usize {
    let mut collisions = 0;
    for (position, &idx) in wave.iter().enumerate() {
        let id = metas[idx].id;
        for &other in &wave[position + 1..] {
            let prev = metas[other].id;
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
