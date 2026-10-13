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

//! How a frame's fuel is shared among behaviors, and what each behavior
//! carries from one frame to the next to keep the sharing fair.
//!
//! Behaviors run in a fixed order, so on one entity the earlier always runs
//! first. Fixed order alone would let the first behaviors take everything; a
//! share each keeps them from it. What a behavior is owed or owes is carried
//! here:
//!
//! - a **share** of the frame's fuel, the same for every behavior that can
//!   spend; the units that do not divide go to each behavior in turn, frame
//!   after frame, so a frame with less fuel than behaviors still reaches them
//!   all;
//! - a **debt**, for a behavior that spent past what it was handed — a turn
//!   cannot stop inside one instruction, so it may. It pays it back from its
//!   next shares, and what it pays passes on to the behaviors after it: one
//!   that overdrafts every frame cannot keep the next out of every frame;
//! - a **cursor**, the first of its instances a frame had no fuel for, where
//!   it starts the next frame.

use std::collections::HashMap;

use khora_core::ecs::entity::EntityId;
use khora_data::flow::{ScriptProgram, ScriptView};

use super::runtime::{Instance, ScriptRuntime};

/// A behavior, by its module and its name.
type Behavior = (String, String);

/// What one behavior is handed of a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Share {
    /// Its share of the frame's fuel.
    pub(super) own: u64,
    /// What of that share goes to its debt — not spent by it, so passed on to
    /// the behaviors after it.
    pub(super) paid: u64,
    /// Whether it still owes once this share is paid: then it runs nothing
    /// this frame, not even on what the behaviors before it passed on.
    pub(super) owing: bool,
}

/// What each behavior carries from one frame to the next.
#[derive(Debug, Default)]
pub struct FuelShares {
    cursors: HashMap<Behavior, EntityId>,
    debts: HashMap<Behavior, u64>,
    /// Which behavior the first unit that does not divide goes to.
    rotation: u64,
}

impl FuelShares {
    /// Shares `pool` among `behaviors`, in order, each with whether it can
    /// spend this frame: `None` for one that cannot — its program is not
    /// loaded, or every instance of it faulted — so that no fuel is set
    /// aside for it.
    ///
    /// Forgets what it carried for a behavior not in `behaviors`: a frame with
    /// no instance of it has nowhere for that behavior to start.
    pub(super) fn plan(
        &mut self,
        behaviors: &[(&str, &str, bool)],
        pool: u64,
    ) -> Vec<Option<Share>> {
        self.cursors
            .retain(|(module, name), _| behaviors.iter().any(|b| (b.0, b.1) == (module, name)));
        self.debts
            .retain(|(module, name), _| behaviors.iter().any(|b| (b.0, b.1) == (module, name)));

        let spending = behaviors.iter().filter(|b| b.2).count() as u64;
        if spending == 0 {
            return vec![None; behaviors.len()];
        }
        let (base, extra) = (pool / spending, pool % spending);
        let first = self.rotation % spending;
        self.rotation = self.rotation.wrapping_add(1);

        let mut nth = 0;
        behaviors
            .iter()
            .map(|&(module, name, spends)| {
                if !spends {
                    return None;
                }
                let turn = (nth + spending - first) % spending;
                nth += 1;
                let own = base + u64::from(turn < extra);
                let key = (module.to_owned(), name.to_owned());
                let debt = self.debts.get(&key).copied().unwrap_or(0);
                let paid = debt.min(own);
                let owing = debt > paid;
                if owing {
                    self.debts.insert(key, debt - paid);
                } else {
                    self.debts.remove(&key);
                }
                Some(Share { own, paid, owing })
            })
            .collect()
    }

    /// Where a behavior whose instances are `entities`, in order, starts this
    /// frame: at its cursor, or — the instance there has gone — at the next
    /// entity after it, so the rotation carries on rather than starting over;
    /// past the last, at the first. Entity order, not the view's: the view may
    /// list one behavior's instances in more than one run.
    pub(super) fn start(&self, module: &str, behavior: &str, entities: &[EntityId]) -> usize {
        let Some(cursor) = self.cursors.get(&(module.to_owned(), behavior.to_owned())) else {
            return 0;
        };
        let order = |entity: &EntityId| (entity.index, entity.generation);
        let next = |after: bool| {
            entities
                .iter()
                .enumerate()
                .filter(|(_, entity)| !after || order(entity) > order(cursor))
                .min_by_key(|(_, entity)| order(entity))
                .map(|(at, _)| at)
        };
        entities
            .iter()
            .position(|entity| entity == cursor)
            .or_else(|| next(true))
            .or_else(|| next(false))
            .unwrap_or(0)
    }

    /// What a behavior leaves for the next frame: where it starts — the first
    /// of its instances this one had no fuel for, or its beginning — and what
    /// it spent past what it was handed, added to its debt.
    pub(super) fn settle(
        &mut self,
        module: &str,
        behavior: &str,
        starved_at: Option<EntityId>,
        overspent: u64,
    ) {
        let key = (module.to_owned(), behavior.to_owned());
        match starved_at {
            Some(entity) => {
                self.cursors.insert(key.clone(), entity);
            }
            None => {
                self.cursors.remove(&key);
            }
        }
        if overspent > 0 {
            *self.debts.entry(key).or_default() += overspent;
        }
    }
}

/// The frame's behaviors in their fixed order — the view's program order —
/// each with its instances in the order it runs them this frame, from its
/// cursor round to it, and its share of `pool`.
pub(super) fn behaviors<'v>(
    view: &'v ScriptView,
    runtime: &mut ScriptRuntime,
    pool: u64,
) -> Vec<(&'v ScriptProgram, Vec<usize>, Option<Share>)> {
    let mut groups: Vec<(u32, Vec<usize>)> = Vec::new();
    for (at, instance) in view.instances.iter().enumerate() {
        match groups
            .iter_mut()
            .find(|(program, _)| *program == instance.program)
        {
            Some((_, members)) => members.push(at),
            None => groups.push((instance.program, vec![at])),
        }
    }
    groups.sort_by_key(|(program, _)| *program);
    let groups: Vec<_> = groups
        .into_iter()
        .filter_map(|(program, members)| Some((view.programs.get(program as usize)?, members)))
        .collect();

    // A behavior that cannot spend — no program, or every instance faulted —
    // is handed nothing, rather than a share nobody spends. An instance
    // arriving this frame starts again, and may run.
    let spends: Vec<_> = groups
        .iter()
        .map(|(program, members)| {
            let enabled = members.iter().any(|at| {
                let instance = &view.instances[*at];
                instance.arrival.is_some()
                    || !idle(runtime.peek(instance.entity, &program.behavior))
            });
            (
                program.module.as_str(),
                program.behavior.as_str(),
                enabled && runtime.program(&program.module).is_some(),
            )
        })
        .collect();
    let shares = runtime.shares.plan(&spends, pool);

    groups
        .into_iter()
        .zip(shares)
        .map(|((program, members), share)| {
            let entities: Vec<_> = members
                .iter()
                .map(|at| view.instances[*at].entity)
                .collect();
            let start = runtime
                .shares
                .start(&program.module, &program.behavior, &entities);
            let order = members[start..]
                .iter()
                .chain(&members[..start])
                .copied()
                .collect();
            (program, order, share)
        })
        .collect()
}

/// Whether an instance will not run whatever the fuel: disabled by a fault,
/// and with no arrival to start it again.
pub(super) fn idle(instance: Option<&Instance>) -> bool {
    matches!(instance, Some(state) if state.disabled && state.arriving.is_none())
}
