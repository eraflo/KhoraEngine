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

//! A generic implementation of Kahn's algorithm for topological sorting.

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

/// An error indicating that a cycle was detected in the graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleError;

/// Performs a topological sort on a generic directed graph.
///
/// The graph is defined by a collection of nodes and a set of directed edges
/// representing dependencies (from parent to child).
///
/// # Type Parameters
///
/// * `T`: The type of the nodes in the graph. It must be hashable and equatable
///   to be used in internal maps.
///
/// # Arguments
///
/// * `nodes`: An iterator over the unique nodes in the graph.
/// * `edges`: An iterator over the directed edges, represented as `(parent, child)` tuples.
///
/// # Returns
///
/// * `Ok(Vec<T>)`: A vector of nodes in a valid topological order.
/// * `Err(CycleError)`: If the graph contains one or more cycles.
pub fn topological_sort<T>(
    nodes: impl IntoIterator<Item = T>,
    edges: impl IntoIterator<Item = (T, T)>,
) -> Result<Vec<T>, CycleError>
where
    T: Copy + Eq + Hash,
{
    let node_list: Vec<T> = nodes.into_iter().collect();
    if node_list.is_empty() {
        return Ok(Vec::new());
    }

    let mut adjacency_list: HashMap<T, Vec<T>> = HashMap::new();
    let mut in_degree: HashMap<T, usize> = node_list.iter().map(|id| (*id, 0)).collect();

    // 1. Build adjacency list and in-degree counts from edges.
    for (parent, child) in edges {
        adjacency_list.entry(parent).or_default().push(child);
        if let Some(degree) = in_degree.get_mut(&child) {
            *degree += 1;
        }
    }

    // 2. Initialize queue with all root nodes (in-degree of 0).
    let mut queue: VecDeque<T> = VecDeque::new();
    for &node in &node_list {
        if in_degree.get(&node).cloned().unwrap_or(0) == 0 {
            queue.push_back(node);
        }
    }

    // 3. Process the queue.
    let mut sorted_list = Vec::with_capacity(node_list.len());
    while let Some(parent_node) = queue.pop_front() {
        sorted_list.push(parent_node);
        if let Some(children) = adjacency_list.get(&parent_node) {
            for &child_node in children {
                if let Some(degree) = in_degree.get_mut(&child_node) {
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(child_node);
                    }
                }
            }
        }
    }

    // 4. Check for cycles.
    if sorted_list.len() != node_list.len() {
        Err(CycleError)
    } else {
        Ok(sorted_list)
    }
}
