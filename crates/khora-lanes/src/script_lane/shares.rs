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
//!
//! What a behavior declares bends the shares, never the order:
//!
//! - a `[Critical]` behavior claims first — what it spent last frame, set
//!   aside before the others share the rest. A pool too short even for that
//!   goes to the critical behaviors alone; the budget is never exceeded;
//! - a `[Budget(d)]` behavior is handed no more than its ceiling, `d` at the
//!   lane's measured rate; what it does not take goes to the others.

use std::collections::HashMap;
use std::sync::Arc;

use khora_core::ecs::entity::EntityId;
use khora_data::flow::{ScriptProgram, ScriptView};
use khora_script::vm::Program;

use super::runtime::{Instance, ScriptRuntime};

/// A behavior, by its module and its name.
type Behavior = (String, String);

/// What a behavior asks of a frame's planning.
#[derive(Debug, Clone, Copy)]
pub(super) struct Claim<'a> {
    /// Its module.
    pub(super) module: &'a str,
    /// Its name.
    pub(super) name: &'a str,
    /// Whether it can spend this frame.
    pub(super) spends: bool,
    /// Whether it is `[Critical]`.
    pub(super) critical: bool,
    /// Its `[Budget(…)]`, in fuel: the most it is handed a frame.
    pub(super) ceiling: Option<u64>,
    /// How many of its instances can run this frame.
    pub(super) instances: u64,
}

/// What one behavior is handed of a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Share {
    /// Its share of the frame's fuel.
    pub(super) own: u64,
    /// The most it may spend this frame, whatever the behaviors before it
    /// pass on.
    pub(super) ceiling: Option<u64>,
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
    /// For each behavior in debt, the instance whose turn overspent.
    debtors: HashMap<Behavior, EntityId>,
    /// What one turn of each behavior cost, the last frame it ran one — a
    /// critical one's claim, and the least a turn is started on.
    turn_cost: HashMap<Behavior, u64>,
}

impl FuelShares {
    /// Shares `pool` among `behaviors`, in order, each with whether it can
    /// spend this frame: `None` for one that cannot — its program is not
    /// loaded, or every instance of it faulted — so that no fuel is set
    /// aside for it.
    ///
    /// Forgets what it carried for a behavior not in `behaviors`: a frame with
    /// no instance of it has nowhere for that behavior to start.
    pub(super) fn plan(&mut self, behaviors: &[Claim<'_>], pool: u64) -> Vec<Option<Share>> {
        let present = |(module, name): &Behavior| {
            behaviors
                .iter()
                .any(|b| (b.module, b.name) == (module.as_str(), name.as_str()))
        };
        self.cursors.retain(|key, _| present(key));
        self.debts.retain(|key, _| present(key));
        self.debtors.retain(|key, _| present(key));
        self.turn_cost.retain(|key, _| present(key));

        let spending: Vec<usize> = (0..behaviors.len())
            .filter(|&at| behaviors[at].spends)
            .collect();
        if spending.is_empty() {
            return vec![None; behaviors.len()];
        }
        let first = self.rotation;
        self.rotation = self.rotation.wrapping_add(1);

        // Critical behaviors claim a turn for each of their instances, at
        // what a turn of theirs cost — never less than an equal share —
        // before the others share what is left.
        let (critical, others): (Vec<usize>, Vec<usize>) = spending
            .iter()
            .copied()
            .partition(|&at| behaviors[at].critical);
        let equal = pool / spending.len() as u64;
        let need = |at: usize| {
            let claim = behaviors[at];
            let key = (claim.module.to_owned(), claim.name.to_owned());
            let turns = self
                .turn_cost
                .get(&key)
                .map_or(0, |cost| cost.saturating_mul(claim.instances));
            let need = turns.max(equal);
            claim.ceiling.map_or(need, |ceiling| need.min(ceiling))
        };
        let needed: u64 = critical.iter().map(|&at| need(at)).sum();
        let mut own = vec![0u64; behaviors.len()];
        if needed >= pool {
            fill(&mut own, &critical, behaviors, pool, first);
        } else {
            for &at in &critical {
                own[at] = need(at);
            }
            fill(&mut own, &others, behaviors, pool - needed, first);
            // What nobody else takes is the critical behaviors' too.
            let handed: u64 = own.iter().sum();
            if handed < pool {
                let mut more = vec![0u64; behaviors.len()];
                fill(&mut more, &critical, behaviors, pool - handed, first);
                for &at in &critical {
                    let ceiling = behaviors[at].ceiling.unwrap_or(u64::MAX);
                    own[at] = own[at].saturating_add(more[at]).min(ceiling);
                }
            }
        }

        (0..behaviors.len())
            .map(|at| {
                let claim = behaviors[at];
                if !claim.spends {
                    return None;
                }
                let own = own[at];
                let key = (claim.module.to_owned(), claim.name.to_owned());
                let debt = self.debts.get(&key).copied().unwrap_or(0);
                let paid = debt.min(own);
                let owing = debt > paid;
                if owing {
                    self.debts.insert(key, debt - paid);
                } else {
                    self.debts.remove(&key);
                    self.debtors.remove(&key);
                }
                Some(Share {
                    own,
                    ceiling: claim.ceiling,
                    paid,
                    owing,
                })
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
    ///
    /// `debtor` is the instance whose turn overspent: the frames the behavior
    /// sits out repaying it are that body's run time, no other's.
    pub(super) fn settle(
        &mut self,
        module: &str,
        behavior: &str,
        starved_at: Option<EntityId>,
        (spent, overspent, turns): (u64, u64, u64),
        debtor: Option<EntityId>,
    ) {
        let key = (module.to_owned(), behavior.to_owned());
        if let Some(cost) = spent.checked_div(turns) {
            self.turn_cost.insert(key.clone(), cost);
        }
        match starved_at {
            Some(entity) => {
                self.cursors.insert(key.clone(), entity);
            }
            None => {
                self.cursors.remove(&key);
            }
        }
        if overspent > 0 {
            *self.debts.entry(key.clone()).or_default() += overspent;
            if let Some(debtor) = debtor {
                self.debtors.insert(key, debtor);
            }
        }
    }

    /// Forgives `module`'s `behavior` the debt `entity` ran up: a load brought
    /// that instance back as the save had it, owing nothing.
    pub(super) fn forgive(&mut self, module: &str, behavior: &str, entity: EntityId) {
        let key = (module.to_owned(), behavior.to_owned());
        if self.debtors.get(&key) == Some(&entity) {
            self.debts.remove(&key);
            self.debtors.remove(&key);
        }
    }

    /// What a turn of `module`'s `behavior` cost, the last frame it ran one;
    /// nothing before its first.
    pub(super) fn turn_cost(&self, module: &str, behavior: &str) -> u64 {
        self.turn_cost
            .get(&(module.to_owned(), behavior.to_owned()))
            .copied()
            .unwrap_or(0)
    }

    /// Whether `remaining` is too little to start another turn of
    /// `module`'s `behavior`, `turns` of it run this frame already: less than
    /// half of what one costs. An instance always handed the frame's remainder
    /// would never finish; it starts the next frame instead.
    pub(super) fn too_short(
        &self,
        module: &str,
        behavior: &str,
        turns: u64,
        remaining: u64,
    ) -> bool {
        turns > 0 && remaining.saturating_mul(2) < self.turn_cost(module, behavior)
    }

    /// The instance whose overspending `module`'s `behavior` is repaying.
    pub(super) fn debtor(&self, module: &str, behavior: &str) -> Option<EntityId> {
        self.debtors
            .get(&(module.to_owned(), behavior.to_owned()))
            .copied()
    }
}

/// The fuel mark a behavior with `share` stops at, starting at `spent_before`:
/// `share_end`, but a budgeted behavior spends no more than its ceiling,
/// whatever the behaviors before it passed on.
pub(super) fn capped(share: Option<Share>, share_end: u64, spent_before: u64) -> u64 {
    match share.and_then(|share| share.ceiling) {
        Some(ceiling) => share_end.min(spent_before.saturating_add(ceiling)),
        None => share_end,
    }
}

/// Shares `pool` equally among `members`, each no more than its ceiling —
/// what a capped one cannot take goes to the others; the units that do not
/// divide go to each in turn, from `first`, frame after frame.
fn fill(own: &mut [u64], members: &[usize], behaviors: &[Claim<'_>], pool: u64, first: u64) {
    let mut open: Vec<usize> = members.to_vec();
    let mut left = pool;
    while !open.is_empty() && left > 0 {
        let count = open.len() as u64;
        let base = left / count;
        let capped: Vec<usize> = open
            .iter()
            .copied()
            .filter(|&at| behaviors[at].ceiling.is_some_and(|ceiling| ceiling <= base))
            .collect();
        if capped.is_empty() {
            let extra = left % count;
            let start = first % count;
            for (nth, &at) in open.iter().enumerate() {
                let turn = (nth as u64 + count - start) % count;
                own[at] = base + u64::from(turn < extra);
            }
            return;
        }
        for at in capped {
            let ceiling = behaviors[at].ceiling.unwrap_or(0);
            own[at] = ceiling;
            left -= ceiling;
            open.retain(|&other| other != at);
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
    let rate = runtime.rate();
    let spends: Vec<_> = groups
        .iter()
        .map(|(program, members)| {
            let able = |at: &&usize| {
                let instance = &view.instances[**at];
                instance.arrival.is_some()
                    || !idle(
                        runtime.peek(instance.entity, &program.behavior),
                        runtime.program(&program.module),
                    )
            };
            let enabled = members.iter().any(|at| able(&at));
            // What it claims is a turn for each instance that takes one this
            // frame — a rate-limited one skips most of them.
            let runnable = members
                .iter()
                .filter(able)
                .filter(|at| {
                    super::rate::due(runtime, &view.instances[**at], program, view.delta_seconds)
                })
                .count() as u64;
            let schedule = super::rate::schedule_of(runtime, program);
            Claim {
                module: program.module.as_str(),
                name: program.behavior.as_str(),
                spends: enabled && runtime.program(&program.module).is_some(),
                critical: schedule.critical,
                instances: runnable,
                // Its budget, in the fuel the lane measures a millisecond buys.
                ceiling: schedule
                    .budget
                    .map(|seconds| (f64::from(seconds) * 1000.0 * rate).floor() as u64),
            }
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
/// with no arrival to start it again, and already laid out for `program` —
/// carried into another, an edit's answer to the fault, it runs again.
pub(super) fn idle(instance: Option<&Instance>, program: Option<&Arc<Program>>) -> bool {
    matches!(instance, Some(state)
    if state.disabled
        && state.arriving.is_none()
        && match (&state.built_for, program) {
            (Some(built), Some(program)) => Arc::ptr_eq(built, program),
            _ => true,
        })
}
