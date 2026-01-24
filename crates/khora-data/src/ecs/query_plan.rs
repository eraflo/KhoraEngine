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

//! Defines the `QueryPlan`, which pre-calculates the most efficient way to execute an ECS query.

use crate::ecs::SemanticDomain;
use std::{any::TypeId, collections::HashSet};

/// Defines whether a query can be executed "natively" or requires a transversal join.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryMode {
    /// Optimal path: all requested components are in the same domain.
    /// Iteration happens linearly over matching `ComponentPage`s.
    Native,
    /// Join path: requested components span multiple domains.
    /// Iteration is "driven" by one domain and peers are looked up via bitsets.
    Transversal,
}

/// A pre-calculated execution strategy for a specific set of component types.
///
/// This plan is cached in the `World` to avoid re-analyzing query signatures
/// on every frame.
#[derive(Debug, Clone)]
pub struct QueryPlan {
    /// The execution mode (Native or Transversal).
    pub mode: QueryMode,
    /// If Transversal, this is the domain that drives the iteration.
    pub driver_domain: Option<SemanticDomain>,
    /// If Transversal, these are the peer domains that must be joined.
    pub peer_domains: HashSet<SemanticDomain>,
    /// The signature used to find matching pages (driver domain components).
    pub driver_signature: Vec<TypeId>,
}

impl QueryPlan {
    /// Creates a new `QueryPlan` for the given component types.
    pub fn new(
        is_transversal: bool,
        driver_domain: Option<SemanticDomain>,
        peer_domains: HashSet<SemanticDomain>,
        driver_signature: Vec<TypeId>,
    ) -> Self {
        Self {
            mode: if is_transversal {
                QueryMode::Transversal
            } else {
                QueryMode::Native
            },
            driver_domain,
            peer_domains,
            driver_signature,
        }
    }
}
