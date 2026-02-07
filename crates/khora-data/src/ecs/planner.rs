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

//! Internal query planning and caching.

use crate::ecs::QueryPlan;
use std::any::TypeId;
use std::collections::HashMap;
use std::sync::RwLock;

/// Internal manager for query plans and strategies.
///
/// The `QueryPlanner` acts as a strategic brain for the ECS, analyzing component
/// query signatures to determine the most efficient execution path. It caches
/// generated `QueryPlan`s to avoid redundant analysis for frequently used queries.
pub(crate) struct QueryPlanner {
    /// A thread-safe cache mapping from a set of component `TypeId`s to an optimized `QueryPlan`.
    pub(crate) query_cache: RwLock<HashMap<Vec<TypeId>, QueryPlan>>,
}

impl QueryPlanner {
    /// Creates a new `QueryPlanner` with an empty query cache.
    pub fn new() -> Self {
        Self {
            query_cache: RwLock::new(HashMap::new()),
        }
    }
}
