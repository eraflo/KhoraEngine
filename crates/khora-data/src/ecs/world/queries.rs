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

//! Building queries: planning, page matching, the domain bitset.

use std::{any::TypeId, collections::HashSet};

use khora_core::sync::RwLockExt;

use super::World;
use crate::ecs::{
    query::{NativeRowPlan, Query, ReadOnlyWorldQuery, WorldQuery},
    ComponentKey, DomainBitset, QueryMut, QueryPlan,
};

/// Returns the plan-cache key of a query over the Rust components `type_ids`.
fn plan_key(type_ids: &[TypeId]) -> Vec<ComponentKey> {
    type_ids.iter().copied().map(ComponentKey::Rust).collect()
}

impl World {
    /// Returns an iterator over the entities matching the read-only query `Q`.
    ///
    /// `Q` is a read-only term or a tuple of them: `&T`, `Option<&T>`,
    /// `EntityId`, `Without<T>`, `Changed<T>`, `Soa<T>`. See the book: concepts/ecs.md,
    /// "Queries and SoA versus AoSoA".
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Find all entities with a `Transform` and `GlobalTransform`.
    /// for (transform, global) in world.query::<(&Transform, &GlobalTransform)>() {
    ///     // ...
    /// }
    ///
    /// // Find all root entities (those with a `Transform` but without a `Parent`).
    /// for (transform,) in world.query::<(&Transform, Without<Parent>)>() {
    ///     // ...
    /// }
    /// ```
    pub fn query<'a, Q: ReadOnlyWorldQuery>(&'a self) -> Query<'a, Q> {
        let type_ids = Q::type_ids();

        // The plan is cached, never the pages: they are matched on every call.
        let plan = {
            let cache = self.planner.query_cache.read_recovered();
            if let Some(plan) = cache.get(&plan_key(&type_ids)) {
                plan.clone()
            } else {
                drop(cache);
                let new_plan = self.analyze_query(&type_ids);
                let mut cache = self.planner.query_cache.write_recovered();
                cache.insert(plan_key(&type_ids), new_plan.clone());
                new_plan
            }
        };

        let without_type_ids = Q::without_type_ids();
        let matching_page_indices = self.plan_pages(&plan, &without_type_ids);
        let row_plan = self.native_row_plan(&plan, &without_type_ids, &Q::optional_type_ids());

        // One access observation per query, off the per-row path, for layout adaptation.
        let rows_scanned: u64 = matching_page_indices
            .iter()
            .map(|&pid| self.storage.pages[pid as usize].row_count() as u64)
            .sum();
        self.storage
            .registry
            .record_access(&plan_key(&type_ids), rows_scanned);

        Query::new(self, plan, matching_page_indices, row_plan)
    }

    /// Returns an iterator over the entities matching query `Q`, whose `&mut T`
    /// terms yield `Mut` guards.
    ///
    /// # Panics
    ///
    /// If `Q` names one component twice, as `(&mut Position, &Position)`: it
    /// would hand out two references to one value.
    pub fn query_mut<'a, Q: WorldQuery>(&'a mut self) -> QueryMut<'a, Q> {
        let accessed = Q::accessed_type_ids();
        let mut seen = HashSet::with_capacity(accessed.len());
        if !accessed.iter().all(|id| seen.insert(*id)) {
            panic!(
                "`query_mut::<{}>` names one component twice: it would hand out two references \
                 to one value",
                std::any::type_name::<Q>()
            );
        }
        let type_ids = Q::type_ids();

        let plan = {
            let cache = self.planner.query_cache.read_recovered();
            if let Some(plan) = cache.get(&plan_key(&type_ids)) {
                plan.clone()
            } else {
                drop(cache);
                let new_plan = self.analyze_query(&type_ids);
                let mut cache = self.planner.query_cache.write_recovered();
                cache.insert(plan_key(&type_ids), new_plan.clone());
                new_plan
            }
        };

        let without_type_ids = Q::without_type_ids();
        let matching_page_indices = self.plan_pages(&plan, &without_type_ids);
        let row_plan = self.native_row_plan(&plan, &without_type_ids, &Q::optional_type_ids());

        let rows_scanned: u64 = matching_page_indices
            .iter()
            .map(|&pid| self.storage.pages[pid as usize].row_count() as u64)
            .sum();
        self.storage
            .registry
            .record_access(&plan_key(&type_ids), rows_scanned);

        QueryMut::new(self, plan, matching_page_indices, row_plan)
    }

    /// Returns the execution plan of a query over `type_ids`.
    ///
    /// A query spanning several domains is transversal, driven by the densest
    /// domain (most entities per page).
    pub(crate) fn analyze_query(&self, type_ids: &[TypeId]) -> QueryPlan {
        if type_ids.is_empty() {
            // No component term: every page would match, yielding an entity
            // once per row it owns, orphans included.
            return QueryPlan::entity_scan();
        }

        let mut domains = HashSet::new();
        for type_id in type_ids {
            if let Some(domain) = self.storage.registry.get_domain(*type_id) {
                domains.insert(domain);
            }
        }

        if domains.len() <= 1 {
            let first_domain = domains.into_iter().next();
            let plan = QueryPlan::new(false, first_domain, HashSet::new(), plan_key(type_ids));
            return plan;
        }

        // Densest first: entities_a / pages_a > entities_b / pages_b, cross-multiplied.
        let driver_domain = domains
            .iter()
            .min_by(|&&a, &&b| {
                let stats_a = self
                    .storage
                    .domain_stats
                    .get(&a)
                    .copied()
                    .unwrap_or_default();
                let stats_b = self
                    .storage
                    .domain_stats
                    .get(&b)
                    .copied()
                    .unwrap_or_default();
                let score_a = (stats_a.page_count as u64) * (stats_b.entity_count as u64);
                let score_b = (stats_b.page_count as u64) * (stats_a.entity_count as u64);
                score_a.cmp(&score_b)
            })
            .copied()
            .unwrap(); // `domains` holds at least two.

        let mut peer_domains = domains;
        peer_domains.remove(&driver_domain);

        let mut driver_signature = Vec::new();
        for type_id in type_ids {
            if self.storage.registry.get_domain(*type_id) == Some(driver_domain) {
                driver_signature.push(ComponentKey::Rust(*type_id));
            }
        }
        driver_signature.sort();

        QueryPlan::new(true, Some(driver_domain), peer_domains, driver_signature)
    }

    /// Returns the pages a query iterates, holding its driver signature and no `without` type.
    ///
    /// None for an entity scan, which walks the entity store.
    fn plan_pages(&self, plan: &QueryPlan, without_type_ids: &[TypeId]) -> Vec<u32> {
        if plan.mode == crate::ecs::QueryMode::EntityScan {
            return Vec::new();
        }
        self.find_matching_pages(&plan.driver_signature, without_type_ids)
    }

    /// Returns the per-row work a Native iteration needs beyond its driver row.
    ///
    /// Page matching and the page-row fetch see only the driver domain's page,
    /// so only `Without` and `Option` terms of another domain cost per-row
    /// work. Other modes get the empty plan: they already join every term.
    pub(crate) fn native_row_plan(
        &self,
        plan: &QueryPlan,
        without_type_ids: &[TypeId],
        optional_type_ids: &[TypeId],
    ) -> NativeRowPlan {
        if plan.mode != crate::ecs::QueryMode::Native {
            return NativeRowPlan::default();
        }
        // No driver domain: every term is unregistered, so none is foreign.
        let Some(driver_domain) = plan.driver_domain else {
            return NativeRowPlan::default();
        };
        // An unregistered type has no domain, so it is never foreign.
        let foreign_domain = |type_id: TypeId| {
            self.storage
                .registry
                .get_domain(type_id)
                .filter(|&domain| domain != driver_domain)
        };
        if optional_type_ids
            .iter()
            .any(|&type_id| foreign_domain(type_id).is_some())
        {
            return NativeRowPlan {
                foreign_without: Vec::new(),
                fetch_from_world: true,
            };
        }
        NativeRowPlan {
            foreign_without: without_type_ids
                .iter()
                .filter_map(|&type_id| Some((foreign_domain(type_id)?, type_id)))
                .collect(),
            fetch_from_world: false,
        }
    }

    /// Returns the pages holding every key of `signature` and none of `without_type_ids`.
    fn find_matching_pages(
        &self,
        signature: &[ComponentKey],
        without_type_ids: &[TypeId],
    ) -> Vec<u32> {
        let mut matching_page_indices = Vec::new();
        'page_loop: for (page_id, page) in self.storage.pages.iter().enumerate() {
            for required in signature {
                if page.keys.binary_search(required).is_err() {
                    continue 'page_loop;
                }
            }
            for excluded_type in without_type_ids {
                if page
                    .keys
                    .binary_search(&ComponentKey::Rust(*excluded_type))
                    .is_ok()
                {
                    continue 'page_loop;
                }
            }
            matching_page_indices.push(page_id as u32);
        }
        matching_page_indices
    }

    /// Returns the intersection of a transversal query's domain bitsets, `None` for another mode.
    ///
    /// A join skips the entities outside it without a metadata lookup.
    pub(crate) fn compute_query_bitset(&self, plan: &QueryPlan) -> Option<DomainBitset> {
        if plan.mode != crate::ecs::QueryMode::Transversal {
            return None;
        }

        let driver_domain = plan.driver_domain?;

        let mut bitset = self.storage.domain_bitsets.get(&driver_domain)?.clone();

        for peer_domain in &plan.peer_domains {
            if let Some(peer_bitset) = self.storage.domain_bitsets.get(peer_domain) {
                bitset.intersect(peer_bitset);
            } else {
                return Some(DomainBitset::new());
            }
        }

        Some(bitset)
    }
}
