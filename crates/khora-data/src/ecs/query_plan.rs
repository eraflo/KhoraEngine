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

//! The `QueryPlan`: how a query's signature is iterated, decided once and cached.

use crate::ecs::{ComponentKey, SemanticDomain};
use std::collections::HashSet;

/// How a query iterates: within one domain, across domains, or over entities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryMode {
    /// All requested components are in one domain: the matching pages are walked in order.
    Native,
    /// The components span several domains: one domain drives, peers are joined by bitset.
    Transversal,
    /// The query names no component, so no page can drive it: each live entity is visited once.
    EntityScan,
}

/// How a query over one set of component types iterates; cached in the `World`.
#[derive(Debug, Clone)]
pub struct QueryPlan {
    /// The execution mode.
    pub mode: QueryMode,
    /// If Transversal, this is the domain that drives the iteration.
    pub driver_domain: Option<SemanticDomain>,
    /// If Transversal, these are the peer domains that must be joined.
    pub peer_domains: HashSet<SemanticDomain>,
    /// The signature used to find matching pages (driver domain components).
    pub driver_signature: Vec<ComponentKey>,
}

impl QueryPlan {
    /// Creates a new `QueryPlan` for the given component types.
    pub fn new(
        is_transversal: bool,
        driver_domain: Option<SemanticDomain>,
        peer_domains: HashSet<SemanticDomain>,
        driver_signature: Vec<ComponentKey>,
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

    /// Creates the plan for a query that names no component (see
    /// [`QueryMode::EntityScan`]).
    pub fn entity_scan() -> Self {
        Self {
            mode: QueryMode::EntityScan,
            driver_domain: None,
            peer_domains: HashSet::new(),
            driver_signature: Vec::new(),
        }
    }
}
