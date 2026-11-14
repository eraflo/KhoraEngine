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

//! The order the scheduler runs the phases in: the built-in ones with the
//! application's own phases added and its removed ones taken out.

use khora_core::agent::ExecutionPhase;

/// Returns the order the scheduler runs the phases in: the built-in ones and
/// the app's own, without those it removed.
///
/// The app's phases run after `OUTPUT` and before `FINALIZE`, in the order it
/// declared them; a phase declared twice runs once.
pub(super) fn phase_order(
    custom: &[ExecutionPhase],
    removed: &[ExecutionPhase],
) -> Vec<ExecutionPhase> {
    let mut order = ExecutionPhase::DEFAULT_ORDER.to_vec();
    let mut at = order
        .iter()
        .position(|p| *p == ExecutionPhase::OUTPUT)
        .map_or(order.len(), |output| output + 1);
    for phase in custom {
        if !order.contains(phase) {
            order.insert(at, *phase);
            at += 1;
        }
    }
    order.retain(|phase| !removed.contains(phase));
    order
}

#[cfg(test)]
mod tests {
    use super::phase_order;
    use khora_core::agent::ExecutionPhase;

    const FIRST: ExecutionPhase = ExecutionPhase::custom(6);
    const SECOND: ExecutionPhase = ExecutionPhase::custom(7);

    /// A game's own phases run after `OUTPUT` and before `FINALIZE`, in the
    /// order the game declared them.
    #[test]
    fn a_games_phases_run_in_the_order_it_declared_them() {
        let order = phase_order(&[FIRST, SECOND], &[]);

        assert_eq!(
            order,
            vec![
                ExecutionPhase::INIT,
                ExecutionPhase::OBSERVE,
                ExecutionPhase::TRANSFORM,
                ExecutionPhase::MUTATE,
                ExecutionPhase::OUTPUT,
                FIRST,
                SECOND,
                ExecutionPhase::FINALIZE,
            ]
        );
    }

    /// A removed phase is not in the order, a built-in one or the game's own.
    #[test]
    fn a_removed_phase_leaves_the_order() {
        let order = phase_order(&[FIRST, SECOND], &[ExecutionPhase::OBSERVE, FIRST]);

        assert!(!order.contains(&ExecutionPhase::OBSERVE));
        assert!(!order.contains(&FIRST));
        assert_eq!(
            order,
            vec![
                ExecutionPhase::INIT,
                ExecutionPhase::TRANSFORM,
                ExecutionPhase::MUTATE,
                ExecutionPhase::OUTPUT,
                SECOND,
                ExecutionPhase::FINALIZE,
            ]
        );
    }

    /// A phase declared twice, or a built-in phase declared again, runs once.
    #[test]
    fn a_phase_declared_twice_runs_once() {
        let order = phase_order(&[FIRST, FIRST, ExecutionPhase::OBSERVE], &[]);

        assert_eq!(order.iter().filter(|p| **p == FIRST).count(), 1);
        assert_eq!(
            order
                .iter()
                .filter(|p| **p == ExecutionPhase::OBSERVE)
                .count(),
            1
        );
    }
}
