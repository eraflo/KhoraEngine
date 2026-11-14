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

//! The whole of what a read did, as a tree that mirrors the record.

use super::watch::{Event, Step, Watcher};

/// One value the reader went through.
#[derive(Debug, Default)]
pub(super) struct Node {
    pub events: Vec<Event>,
    pub children: Vec<(Step, usize)>,
}

/// The whole read, as a tree rooted at node 0.
#[derive(Debug)]
pub(super) struct Tree {
    pub nodes: Vec<Node>,
}

impl Tree {
    pub fn new() -> Self {
        Self {
            nodes: vec![Node::default()],
        }
    }
}

impl Watcher for Tree {
    fn child(&mut self, parent: usize, step: Step) -> usize {
        let id = self.nodes.len();
        self.nodes.push(Node::default());
        if let Some(node) = self.nodes.get_mut(parent) {
            node.children.push((step, id));
        }
        id
    }

    fn note(&mut self, node: usize, event: Event) {
        if let Some(node) = self.nodes.get_mut(node) {
            node.events.push(event);
        }
    }
}
