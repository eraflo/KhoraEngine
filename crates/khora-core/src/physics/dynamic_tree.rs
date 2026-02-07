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

//! # Dynamic AABB Tree
//!
//! A high-performance spatial partitioning structure for broad-phase collision detection.
//! It uses an incrementally balanced binary tree of AABBs.

use crate::math::Aabb;
use crate::math::Vec3;

const NULL_NODE: i32 = -1;

/// A node in the dynamic tree.
#[derive(Debug, Clone)]
pub struct DynamicTreeNode<T: Clone> {
    /// Enlarged AABB for this node.
    pub aabb: Aabb,
    /// User data (e.g., EntityId or index).
    pub user_data: Option<T>,
    /// Index of the parent node.
    pub parent: i32,
    /// Indices of child nodes (if internal).
    pub children: [i32; 2],
    /// Height of the node in the tree (0 for leaves).
    pub height: i32,
}

impl<T: Clone> DynamicTreeNode<T> {
    /// Returns true if this node is a leaf (has no children).
    #[inline]
    pub fn is_leaf(&self) -> bool {
        self.children[0] == NULL_NODE
    }
}

/// A dynamic AABB tree for efficient spatial queries.
#[derive(Debug, Clone)]
pub struct DynamicTree<T: Clone> {
    root: i32,
    nodes: Vec<DynamicTreeNode<T>>,
    free_list: i32,
    node_count: usize,
}

impl<T: Clone> Default for DynamicTree<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone> DynamicTree<T> {
    /// Creates a new, empty dynamic tree.
    pub fn new() -> Self {
        Self {
            root: NULL_NODE,
            nodes: Vec::new(),
            free_list: NULL_NODE,
            node_count: 0,
        }
    }

    /// Inserts a new leaf into the tree and returns its index.
    pub fn insert(&mut self, aabb: Aabb, user_data: T) -> i32 {
        let leaf = self.allocate_node();
        self.nodes[leaf as usize].aabb = aabb;
        self.nodes[leaf as usize].user_data = Some(user_data);
        self.nodes[leaf as usize].height = 0;

        self.insert_leaf(leaf);
        leaf
    }

    /// Removes a leaf from the tree.
    pub fn remove(&mut self, leaf: i32) {
        debug_assert!(leaf != NULL_NODE);
        debug_assert!(self.nodes[leaf as usize].is_leaf());

        self.remove_leaf(leaf);
        self.deallocate_node(leaf);
    }

    /// Updates a leaf with a new AABB.
    /// If the new AABB is still within the "fat" AABB, no tree update is needed.
    pub fn update(
        &mut self,
        leaf: i32,
        aabb: Aabb,
        displacement: Vec3,
        force_update: bool,
    ) -> bool {
        debug_assert!(leaf != NULL_NODE);
        debug_assert!(self.nodes[leaf as usize].is_leaf());

        if !force_update && self.nodes[leaf as usize].aabb.contains_aabb(&aabb) {
            return false;
        }

        self.remove_leaf(leaf);

        // Fatten the AABB
        let mut fat_aabb = aabb;
        let extension = Vec3::ONE * 0.1; // Magic constant for fattening
        fat_aabb.min = fat_aabb.min - extension;
        fat_aabb.max = fat_aabb.max + extension;

        // Predictive encoding: add displacement to the AABB in the direction of travel
        if displacement.x < 0.0 {
            fat_aabb.min.x += displacement.x * 2.0;
        } else {
            fat_aabb.max.x += displacement.x * 2.0;
        }
        if displacement.y < 0.0 {
            fat_aabb.min.y += displacement.y * 2.0;
        } else {
            fat_aabb.max.y += displacement.y * 2.0;
        }
        if displacement.z < 0.0 {
            fat_aabb.min.z += displacement.z * 2.0;
        } else {
            fat_aabb.max.z += displacement.z * 2.0;
        }

        self.nodes[leaf as usize].aabb = fat_aabb;
        self.insert_leaf(leaf);
        true
    }

    /// Returns the user data for a given leaf.
    pub fn get_user_data(&self, leaf: i32) -> &T {
        self.nodes[leaf as usize].user_data.as_ref().unwrap()
    }

    /// Queries the tree for all pairs of overlapping leaves.
    pub fn query_pairs<F>(&self, mut callback: F)
    where
        F: FnMut(&T, &T),
    {
        if self.root == NULL_NODE {
            return;
        }

        // Simultaneous tree traversal for all pairs.
        let mut stack: Vec<i32> = Vec::with_capacity(256);
        stack.push(self.root);

        while let Some(node_id) = stack.pop() {
            let node = &self.nodes[node_id as usize];
            if node.height < 1 {
                continue;
            }

            let left = node.children[0];
            let right = node.children[1];

            self.query_overlap(left, right, &mut callback);

            stack.push(left);
            stack.push(right);
        }
    }

    fn query_overlap<F>(&self, i_a: i32, i_b: i32, callback: &mut F)
    where
        F: FnMut(&T, &T),
    {
        let node_a = &self.nodes[i_a as usize];
        let node_b = &self.nodes[i_b as usize];

        if !node_a.aabb.intersects_aabb(&node_b.aabb) {
            return;
        }

        if node_a.is_leaf() && node_b.is_leaf() {
            if i_a != i_b {
                callback(
                    node_a.user_data.as_ref().unwrap(),
                    node_b.user_data.as_ref().unwrap(),
                );
            }
        } else if node_a.is_leaf() {
            self.query_overlap(i_a, node_b.children[0], callback);
            self.query_overlap(i_a, node_b.children[1], callback);
        } else if node_b.is_leaf() {
            self.query_overlap(node_a.children[0], i_b, callback);
            self.query_overlap(node_a.children[1], i_b, callback);
        } else {
            self.query_overlap(node_a.children[0], node_b.children[0], callback);
            self.query_overlap(node_a.children[0], node_b.children[1], callback);
            self.query_overlap(node_a.children[1], node_b.children[0], callback);
            self.query_overlap(node_a.children[1], node_b.children[1], callback);
        }
    }

    /// Queries the tree for leaves overlapping the given AABB.
    pub fn query<F>(&self, aabb: &Aabb, mut callback: F)
    where
        F: FnMut(&T) -> bool,
    {
        if self.root == NULL_NODE {
            return;
        }

        let mut stack = Vec::with_capacity(64);
        stack.push(self.root);

        while let Some(node_id) = stack.pop() {
            let node = &self.nodes[node_id as usize];
            if node.aabb.intersects_aabb(aabb) {
                if node.is_leaf() {
                    let proceed = callback(node.user_data.as_ref().unwrap());
                    if !proceed {
                        return;
                    }
                } else {
                    stack.push(node.children[0]);
                    stack.push(node.children[1]);
                }
            }
        }
    }

    // --- Internal Leaf Management ---

    fn insert_leaf(&mut self, leaf: i32) {
        if self.root == NULL_NODE {
            self.root = leaf;
            self.nodes[self.root as usize].parent = NULL_NODE;
            return;
        }

        // Find the best sibling for the new leaf
        let leaf_aabb = self.nodes[leaf as usize].aabb;
        let mut index = self.root;
        while !self.nodes[index as usize].is_leaf() {
            let node = &self.nodes[index as usize];
            let child1 = node.children[0];
            let child2 = node.children[1];

            let area = node.aabb.surface_area();
            let combined_aabb = node.aabb.merge(&leaf_aabb);
            let combined_area = combined_aabb.surface_area();

            // Cost of creating a new parent for this node and the new leaf
            let cost = 2.0 * combined_area;

            // Minimum cost of pushing the leaf further down the tree
            let inheritance_cost = 2.0 * (combined_area - area);

            // Cost of descending into child1
            let cost1 = {
                let child1_node = &self.nodes[child1 as usize];
                let new_area = child1_node.aabb.merge(&leaf_aabb).surface_area();
                if child1_node.is_leaf() {
                    new_area + inheritance_cost
                } else {
                    let old_area = child1_node.aabb.surface_area();
                    (new_area - old_area) + inheritance_cost
                }
            };

            // Cost of descending into child2
            let cost2 = {
                let child2_node = &self.nodes[child2 as usize];
                let new_area = child2_node.aabb.merge(&leaf_aabb).surface_area();
                if child2_node.is_leaf() {
                    new_area + inheritance_cost
                } else {
                    let old_area = child2_node.aabb.surface_area();
                    (new_area - old_area) + inheritance_cost
                }
            };

            // Descend according to the minimum cost
            if cost < cost1 && cost < cost2 {
                break;
            }

            index = if cost1 < cost2 { child1 } else { child2 };
        }

        let sibling = index;

        // Create a new parent
        let old_parent = self.nodes[sibling as usize].parent;
        let new_parent = self.allocate_node();
        self.nodes[new_parent as usize].parent = old_parent;
        self.nodes[new_parent as usize].user_data = None;
        self.nodes[new_parent as usize].aabb = leaf_aabb.merge(&self.nodes[sibling as usize].aabb);
        self.nodes[new_parent as usize].height = self.nodes[sibling as usize].height + 1;

        if old_parent != NULL_NODE {
            // Sibling was not root
            if self.nodes[old_parent as usize].children[0] == sibling {
                self.nodes[old_parent as usize].children[0] = new_parent;
            } else {
                self.nodes[old_parent as usize].children[1] = new_parent;
            }
            self.nodes[new_parent as usize].children[0] = sibling;
            self.nodes[new_parent as usize].children[1] = leaf;
            self.nodes[sibling as usize].parent = new_parent;
            self.nodes[leaf as usize].parent = new_parent;
        } else {
            // Sibling was root
            self.nodes[new_parent as usize].children[0] = sibling;
            self.nodes[new_parent as usize].children[1] = leaf;
            self.nodes[sibling as usize].parent = new_parent;
            self.nodes[leaf as usize].parent = new_parent;
            self.root = new_parent;
        }

        // Walk back up the tree fixing heights and AABBs
        index = self.nodes[leaf as usize].parent;
        while index != NULL_NODE {
            index = self.balance(index);

            let child1 = self.nodes[index as usize].children[0];
            let child2 = self.nodes[index as usize].children[1];

            debug_assert!(child1 != NULL_NODE);
            debug_assert!(child2 != NULL_NODE);

            self.nodes[index as usize].height = 1 + self.nodes[child1 as usize]
                .height
                .max(self.nodes[child2 as usize].height);
            self.nodes[index as usize].aabb = self.nodes[child1 as usize]
                .aabb
                .merge(&self.nodes[child2 as usize].aabb);

            index = self.nodes[index as usize].parent;
        }
    }

    fn remove_leaf(&mut self, leaf: i32) {
        if leaf == self.root {
            self.root = NULL_NODE;
            return;
        }

        let parent = self.nodes[leaf as usize].parent;
        let grand_parent = self.nodes[parent as usize].parent;
        let sibling = if self.nodes[parent as usize].children[0] == leaf {
            self.nodes[parent as usize].children[1]
        } else {
            self.nodes[parent as usize].children[0]
        };

        if grand_parent != NULL_NODE {
            // Destroy parent and connect sibling to grandParent
            if self.nodes[grand_parent as usize].children[0] == parent {
                self.nodes[grand_parent as usize].children[0] = sibling;
            } else {
                self.nodes[grand_parent as usize].children[1] = sibling;
            }
            self.nodes[sibling as usize].parent = grand_parent;
            self.deallocate_node(parent);

            // Adjust ancestor bounds
            let mut index = grand_parent;
            while index != NULL_NODE {
                index = self.balance(index);

                let child1 = self.nodes[index as usize].children[0];
                let child2 = self.nodes[index as usize].children[1];

                self.nodes[index as usize].aabb = self.nodes[child1 as usize]
                    .aabb
                    .merge(&self.nodes[child2 as usize].aabb);
                self.nodes[index as usize].height = 1 + self.nodes[child1 as usize]
                    .height
                    .max(self.nodes[child2 as usize].height);

                index = self.nodes[index as usize].parent;
            }
        } else {
            self.root = sibling;
            self.nodes[sibling as usize].parent = NULL_NODE;
            self.deallocate_node(parent);
        }
    }

    // --- Node Allocation ---

    fn allocate_node(&mut self) -> i32 {
        if self.free_list != NULL_NODE {
            let index = self.free_list;
            self.free_list = self.nodes[index as usize].parent;
            self.nodes[index as usize].parent = NULL_NODE;
            self.nodes[index as usize].children = [NULL_NODE, NULL_NODE];
            self.nodes[index as usize].height = 0;
            self.node_count += 1;
            index
        } else {
            let index = self.nodes.len() as i32;
            self.nodes.push(DynamicTreeNode {
                aabb: Aabb::INVALID,
                user_data: None,
                parent: NULL_NODE,
                children: [NULL_NODE, NULL_NODE],
                height: 0,
            });
            self.node_count += 1;
            index
        }
    }

    fn deallocate_node(&mut self, index: i32) {
        debug_assert!(index != NULL_NODE);
        self.nodes[index as usize].parent = self.free_list;
        self.nodes[index as usize].user_data = None;
        self.free_list = index;
        self.node_count -= 1;
    }

    // --- Balancing (Tree Rotations) ---

    fn balance(&mut self, i_a: i32) -> i32 {
        debug_assert!(i_a != NULL_NODE);

        let node_a = &self.nodes[i_a as usize];
        if node_a.is_leaf() || node_a.height < 2 {
            return i_a;
        }

        let i_b = node_a.children[0];
        let i_c = node_a.children[1];

        let balance = self.nodes[i_c as usize].height - self.nodes[i_b as usize].height;

        // Rotate C up
        if balance > 1 {
            let i_f = self.nodes[i_c as usize].children[0];
            let i_g = self.nodes[i_c as usize].children[1];

            // Swap A and C
            self.nodes[i_c as usize].children[0] = i_a;
            self.nodes[i_c as usize].parent = self.nodes[i_a as usize].parent;
            self.nodes[i_a as usize].parent = i_c;

            // A's old parent should point to C
            if self.nodes[i_c as usize].parent != NULL_NODE {
                let p = self.nodes[i_c as usize].parent;
                if self.nodes[p as usize].children[0] == i_a {
                    self.nodes[p as usize].children[0] = i_c;
                } else {
                    self.nodes[p as usize].children[1] = i_c;
                }
            } else {
                self.root = i_c;
            }

            // Rotate
            if self.nodes[i_f as usize].height > self.nodes[i_g as usize].height {
                self.nodes[i_c as usize].children[1] = i_f;
                self.nodes[i_a as usize].children[1] = i_g;
                self.nodes[i_g as usize].parent = i_a;

                self.update_node_meta(i_a);
                self.update_node_meta(i_c);
            } else {
                self.nodes[i_c as usize].children[1] = i_g;
                self.nodes[i_a as usize].children[1] = i_f;
                self.nodes[i_f as usize].parent = i_a;

                self.update_node_meta(i_a);
                self.update_node_meta(i_c);
            }

            return i_c;
        }

        // Rotate B up
        if balance < -1 {
            let i_d = self.nodes[i_b as usize].children[0];
            let i_e = self.nodes[i_b as usize].children[1];

            // Swap A and B
            self.nodes[i_b as usize].children[0] = i_a;
            self.nodes[i_b as usize].parent = self.nodes[i_a as usize].parent;
            self.nodes[i_a as usize].parent = i_b;

            // A's old parent should point to B
            if self.nodes[i_b as usize].parent != NULL_NODE {
                let p = self.nodes[i_b as usize].parent;
                if self.nodes[p as usize].children[0] == i_a {
                    self.nodes[p as usize].children[0] = i_b;
                } else {
                    self.nodes[p as usize].children[1] = i_b;
                }
            } else {
                self.root = i_b;
            }

            // Rotate
            if self.nodes[i_d as usize].height > self.nodes[i_e as usize].height {
                self.nodes[i_b as usize].children[1] = i_d;
                self.nodes[i_a as usize].children[0] = i_e;
                self.nodes[i_e as usize].parent = i_a;

                self.update_node_meta(i_a);
                self.update_node_meta(i_b);
            } else {
                self.nodes[i_b as usize].children[1] = i_e;
                self.nodes[i_a as usize].children[0] = i_d;
                self.nodes[i_d as usize].parent = i_a;

                self.update_node_meta(i_a);
                self.update_node_meta(i_b);
            }

            return i_b;
        }

        i_a
    }

    fn update_node_meta(&mut self, index: i32) {
        let child1 = self.nodes[index as usize].children[0];
        let child2 = self.nodes[index as usize].children[1];
        self.nodes[index as usize].aabb = self.nodes[child1 as usize]
            .aabb
            .merge(&self.nodes[child2 as usize].aabb);
        self.nodes[index as usize].height = 1 + self.nodes[child1 as usize]
            .height
            .max(self.nodes[child2 as usize].height);
    }
}

/// Iterator for traversing all user data in the tree's leaves.
pub struct DynamicTreeIterator<'a, T: Clone> {
    tree: &'a DynamicTree<T>,
    stack: Vec<i32>,
}

impl<'a, T: Clone> Iterator for DynamicTreeIterator<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(node_id) = self.stack.pop() {
            let node = &self.tree.nodes[node_id as usize];
            if node.is_leaf() {
                return Some(node.user_data.as_ref().unwrap());
            } else {
                self.stack.push(node.children[0]);
                self.stack.push(node.children[1]);
            }
        }
        None
    }
}
