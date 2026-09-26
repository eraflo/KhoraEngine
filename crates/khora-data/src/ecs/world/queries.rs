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

use super::World;
use crate::ecs::{
    query::{NativeRowPlan, Query, WorldQuery},
    DomainBitset, QueryMut, QueryPlan,
};

impl World {
    /// Creates an iterator that queries the world for entities matching a set of components and filters.
    ///
    /// This is the primary method for reading and writing data in the ECS. The query `Q`
    /// is specified as a tuple via turbofish syntax. It can include component references
    /// (e.g., `&Position`, `&mut Velocity`) and filters (e.g., `Without<Parent>`).
    ///
    /// This method is very cheap to call. It performs an efficient search to identify all
    /// `ComponentPage`s that satisfy the query's criteria. The returned iterator then
    /// efficiently iterates over the data in only those pages.
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
    pub fn query<'a, Q: WorldQuery>(&'a self) -> Query<'a, Q> {
        let type_ids = Q::type_ids();

        // 1. Try to fetch the strategy plan from the cache.
        // We cache the execution logic (Native vs Transversal), not the page indices.
        let plan = {
            let cache = self
                .planner
                .query_cache
                .read()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(plan) = cache.get(&type_ids) {
                plan.clone()
            } else {
                drop(cache);
                let new_plan = self.analyze_query(&type_ids);
                let mut cache = self
                    .planner
                    .query_cache
                    .write()
                    .unwrap_or_else(|e| e.into_inner());
                cache.insert(type_ids.clone(), new_plan.clone());
                new_plan
            }
        };

        // 2. Dynamically find matching pages for this call.
        // This ensures the query is correct even if new archetypes were created
        // in a different domain since the last call.
        let without_type_ids = Q::without_type_ids();
        let matching_page_indices = self.plan_pages(&plan, &without_type_ids);
        let row_plan = self.native_row_plan(&plan, &without_type_ids, &Q::optional_type_ids());

        // Record one access observation per queried component (coarse, off the
        // per-element path): count the query and the rows it scans. The DCC reads
        // these to drive adaptive memory layout — observation only, never control.
        let rows_scanned: u64 = matching_page_indices
            .iter()
            .map(|&pid| self.storage.pages[pid as usize].row_count() as u64)
            .sum();
        self.storage.registry.record_access(&type_ids, rows_scanned);

        // 3. Return the query with the plan and the current matching pages.
        Query::new(self, plan, matching_page_indices, row_plan)
    }

    /// Creates a mutable iterator that queries the world for entities matching a set of components and filters.
    ///
    /// This method is similar to `query`, but it allows mutable access to the components.
    /// It uses the same dynamic plan re-finding to ensure thread-safe consistency.
    pub fn query_mut<'a, Q: WorldQuery>(&'a mut self) -> QueryMut<'a, Q> {
        let type_ids = Q::type_ids();

        // 1. Get strategy from cache
        let plan = {
            let cache = self
                .planner
                .query_cache
                .read()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(plan) = cache.get(&type_ids) {
                plan.clone()
            } else {
                drop(cache);
                let new_plan = self.analyze_query(&type_ids);
                let mut cache = self
                    .planner
                    .query_cache
                    .write()
                    .unwrap_or_else(|e| e.into_inner());
                cache.insert(type_ids.clone(), new_plan.clone());
                new_plan
            }
        };

        // 2. Dynamically find pages
        let without_type_ids = Q::without_type_ids();
        let matching_page_indices = self.plan_pages(&plan, &without_type_ids);
        let row_plan = self.native_row_plan(&plan, &without_type_ids, &Q::optional_type_ids());

        // Record one access observation per queried component (see `query`).
        let rows_scanned: u64 = matching_page_indices
            .iter()
            .map(|&pid| self.storage.pages[pid as usize].row_count() as u64)
            .sum();
        self.storage.registry.record_access(&type_ids, rows_scanned);

        // The caller may write through every `&mut` term of the query:
        // mark those domains changed ONCE here, at construction — never
        // per row, which would put the bump on the iteration hot path.
        for type_id in Q::mutable_type_ids() {
            if let Some(domain) = self.storage.registry.get_domain(type_id) {
                self.bump_domain_epoch(domain);
            }
        }

        // 3. Construct the iterator
        QueryMut::new(self, plan, matching_page_indices, row_plan)
    }

    /// Analyzes a query's component signature to create an optimized execution plan.
    ///
    /// This method identifies if a query is transversal (spanning multiple domains)
    /// and selects the most efficient "Driver Domain" based on entity density.
    pub(crate) fn analyze_query(&self, type_ids: &[TypeId]) -> QueryPlan {
        if type_ids.is_empty() {
            // ENTITY SCAN: no component term, so no page signature can drive the
            // query — every page would match, yielding an entity once per page
            // row it owns (one per domain, plus migration orphans).
            return QueryPlan::entity_scan();
        }

        let mut domains = HashSet::new();
        for type_id in type_ids {
            if let Some(domain) = self.storage.registry.get_domain(*type_id) {
                domains.insert(domain);
            }
        }

        if domains.len() <= 1 {
            // NATIVE MODE: All components belong to the same semantic domain (or none).
            // This is the fastest execution path as it avoids any cross-domain joins.
            let first_domain = domains.into_iter().next();
            let plan = QueryPlan::new(false, first_domain, HashSet::new(), type_ids.to_vec());
            return plan;
        }

        // TRANSVERSAL MODE
        // Select the domain with the highest density of entities as the driver domain.
        // Highest density = most entities per page => page_A_count * entity_B_count < page_B_count * entity_A_count
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
            .unwrap(); // Should always be Some if domains.len() > 1

        let mut peer_domains = domains;
        peer_domains.remove(&driver_domain);

        // Calculate driver signature (subset of type_ids in the driver domain)
        let mut driver_signature = Vec::new();
        for type_id in type_ids {
            if self.storage.registry.get_domain(*type_id) == Some(driver_domain) {
                driver_signature.push(*type_id);
            }
        }
        driver_signature.sort();

        // Initialize the final plan.
        // In transversal mode, the driver signature is the subset of components
        // that belong to the driver domain.
        QueryPlan::new(true, Some(driver_domain), peer_domains, driver_signature)
    }

    /// Finds the pages a query iterates: none for an entity scan (it walks the
    /// entity store), otherwise the pages matching the plan's driver signature
    /// and not containing any `without` type.
    fn plan_pages(&self, plan: &QueryPlan, without_type_ids: &[TypeId]) -> Vec<u32> {
        if plan.mode == crate::ecs::QueryMode::EntityScan {
            return Vec::new();
        }
        self.find_matching_pages(&plan.driver_signature, without_type_ids)
    }

    /// Decides, once per query, the per-row work a Native iteration needs beyond
    /// its driver row (see [`NativeRowPlan`]).
    ///
    /// `find_matching_pages` excludes a `without` type only at page level, and
    /// the page-row `fetch` reads an `Option` term from the driver row's page:
    /// both are exact for the driver domain (the live driver row's page holds
    /// that domain's components) but blind to a type registered in another
    /// domain, stored in another page. Only those foreign terms cost per-row
    /// work; a query whose terms all share the driver domain gets the empty
    /// plan. Transversal and entity-scan queries get it too: they already join
    /// every term through `fetch_from_world`, which checks every live location.
    pub(crate) fn native_row_plan(
        &self,
        plan: &QueryPlan,
        without_type_ids: &[TypeId],
        optional_type_ids: &[TypeId],
    ) -> NativeRowPlan {
        if plan.mode != crate::ecs::QueryMode::Native {
            return NativeRowPlan::default();
        }
        // No driver domain: every component term is unregistered (a spawned
        // bundle may store such components, which no domain tracks), so there
        // is no domain to be foreign to — keep the page-row path.
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

    /// Internal helper to find pages matching a signature and filter.
    fn find_matching_pages(&self, type_ids: &[TypeId], without_type_ids: &[TypeId]) -> Vec<u32> {
        let mut matching_page_indices = Vec::new();
        'page_loop: for (page_id, page) in self.storage.pages.iter().enumerate() {
            for required_type in type_ids {
                if page.type_ids.binary_search(required_type).is_err() {
                    continue 'page_loop;
                }
            }
            for excluded_type in without_type_ids {
                if page.type_ids.binary_search(excluded_type).is_ok() {
                    continue 'page_loop;
                }
            }
            matching_page_indices.push(page_id as u32);
        }
        matching_page_indices
    }

    /// (Internal) Computes a bitset that represents the intersection of all domains
    /// involved in a transversal query. This is used to speed up joins by skipping
    /// metadata lookups for entities that are guaranteed to not satisfy the query.
    pub(crate) fn compute_query_bitset(&self, plan: &QueryPlan) -> Option<DomainBitset> {
        if plan.mode != crate::ecs::QueryMode::Transversal {
            return None;
        }

        let driver_domain = plan.driver_domain?;

        // Start with the driver domain's bitset.
        let mut bitset = self.storage.domain_bitsets.get(&driver_domain)?.clone();

        // Intersect with all peer domains.
        for peer_domain in &plan.peer_domains {
            if let Some(peer_bitset) = self.storage.domain_bitsets.get(peer_domain) {
                bitset.intersect(peer_bitset);
            } else {
                // If a required peer domain has NO components at all, the intersection is empty.
                return Some(DomainBitset::new());
            }
        }

        Some(bitset)
    }
}
