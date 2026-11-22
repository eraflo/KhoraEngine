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

//! The boot check of the families' `after` lists: every family a family
//! runs after must be able to complete its run earlier in the same frame.
//!
//! The scheduler runs a family only once every family its `after` names has
//! completed a run earlier in the frame. A frame runs the fixed-step rounds
//! first, each round phase by phase, then every other family phase by phase,
//! in the scheduler's order; a family at the background runs off the frame.
//! See the book: concepts/the-frame.md, "Phases and waves".

use super::execution_phase::ExecutionPhase;
use super::registration::{AgentRegistration, BootError};

/// Refuses an `after` the scheduler could never honour.
///
/// Expects every family's phase to be in `phase_order` or the background, as
/// the checks before it make sure.
///
/// # Errors
///
/// The first refusal found, family by family in registration order: an
/// [`AfterItself`](BootError::AfterItself), an
/// [`AfterUnknownFamily`](BootError::AfterUnknownFamily), an
/// [`AfterAtBackground`](BootError::AfterAtBackground), an
/// [`AfterBackgroundFamily`](BootError::AfterBackgroundFamily), an
/// [`AfterOncePerFrameFamily`](BootError::AfterOncePerFrameFamily) or an
/// [`AfterLaterPhase`](BootError::AfterLaterPhase); then an
/// [`AfterCycle`](BootError::AfterCycle).
pub(super) fn check_after(
    registrations: &[&AgentRegistration],
    phase_order: &[ExecutionPhase],
) -> Result<(), BootError> {
    let place = |phase: ExecutionPhase| phase_order.iter().position(|p| *p == phase);
    // Per family, the families it runs after, as indices into `registrations`.
    let mut edges: Vec<Vec<usize>> = Vec::with_capacity(registrations.len());
    for registration in registrations {
        let family = registration.family_name;
        let mut targets = Vec::with_capacity(registration.after.len());
        for &agent in registration.after {
            if agent == registration.agent {
                return Err(BootError::AfterItself { family });
            }
            let Some(index) = registrations.iter().position(|r| r.agent == agent) else {
                return Err(BootError::AfterUnknownFamily {
                    family,
                    target: agent,
                });
            };
            let target = registrations[index];
            if registration.phase == ExecutionPhase::BACKGROUND {
                return Err(BootError::AfterAtBackground {
                    family,
                    target: target.family_name,
                });
            }
            if target.phase == ExecutionPhase::BACKGROUND {
                return Err(BootError::AfterBackgroundFamily {
                    family,
                    target: target.family_name,
                });
            }
            let fixed = registration.fixed_step.is_some();
            let target_fixed = target.fixed_step.is_some();
            if fixed && !target_fixed {
                return Err(BootError::AfterOncePerFrameFamily {
                    family,
                    target: target.family_name,
                });
            }
            // Every fixed-step round runs before the once-per-frame phases,
            // whatever the target's phase.
            let earlier = (!fixed && target_fixed)
                || matches!(
                    (place(registration.phase), place(target.phase)),
                    (Some(own), Some(theirs)) if theirs <= own
                );
            if !earlier {
                return Err(BootError::AfterLaterPhase {
                    family,
                    phase: registration.phase,
                    target: target.family_name,
                    target_phase: target.phase,
                });
            }
            targets.push(index);
        }
        edges.push(targets);
    }
    match find_cycle(&edges) {
        Some(cycle) => Err(BootError::AfterCycle {
            cycle: cycle
                .into_iter()
                .map(|index| registrations[index].family_name)
                .collect(),
        }),
        None => Ok(()),
    }
}

/// Returns the first cycle of `edges`, its first node repeated last.
///
/// Searches depth first from each node in order, and each node's edges in order.
fn find_cycle(edges: &[Vec<usize>]) -> Option<Vec<usize>> {
    /// Where the search stands on a node.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Mark {
        Unseen,
        OnPath,
        Done,
    }

    fn visit(node: usize, edges: &[Vec<usize>], marks: &mut [Mark], path: &mut Vec<usize>) -> bool {
        marks[node] = Mark::OnPath;
        path.push(node);
        for &next in &edges[node] {
            let mark = marks[next];
            if mark == Mark::OnPath {
                let start = path.iter().position(|&n| n == next).unwrap_or(0);
                path.drain(..start);
                path.push(next);
                return true;
            }
            if mark == Mark::Unseen && visit(next, edges, marks, path) {
                return true;
            }
        }
        path.pop();
        marks[node] = Mark::Done;
        false
    }

    let mut marks = vec![Mark::Unseen; edges.len()];
    for node in 0..edges.len() {
        if marks[node] != Mark::Unseen {
            continue;
        }
        let mut path = Vec::new();
        if visit(node, edges, &mut marks, &mut path) {
            return Some(path);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::find_cycle;

    #[test]
    fn find_cycle_finds_none_in_a_chain() {
        assert_eq!(find_cycle(&[vec![], vec![0], vec![1, 0]]), None);
    }

    #[test]
    fn find_cycle_names_the_cycle_alone_from_its_first_node() {
        // 0 runs after 1, which runs after 2, which runs after 1.
        assert_eq!(
            find_cycle(&[vec![1], vec![2], vec![1]]),
            Some(vec![1, 2, 1])
        );
    }
}
