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

//! What a read did, noted as it happens.
//!
//! Only the reader knows whether a field it met was used or skipped, whether a
//! number reached a wider type, which variant name a save used. Guessing those
//! afterwards from values goes wrong exactly where it matters — a dropped
//! `false` looks like a defaulted `false`. So the reader notes them, on a tree
//! that mirrors the record, and the report is read off that tree.

use super::Record;

/// How the reader got from a value to one inside it.
#[derive(Debug, Clone)]
pub(super) enum Step {
    /// A struct field, by the name the save used.
    Field(String),
    /// An element of a sequence.
    Index(usize),
    /// A map entry's value, by its key as the save wrote it.
    Entry(Record),
    /// A map entry's key.
    KeyOf,
    /// An enum variant, by the name the save used.
    Variant(String),
}

/// Something the reader did at one value.
#[derive(Debug, Clone)]
pub(super) enum Event {
    /// Read as a struct declaring these names (canonical names and aliases).
    Fields(&'static [&'static str]),
    /// Skipped: the code has nowhere to put it.
    Ignored,
    /// A number read into a wider kind than it was written as.
    Widened,
    /// A struct read from a sequence: its fields by position.
    Positional,
}

/// Where the reader notes what it does.
pub(super) trait Watcher {
    /// The node for the value reached from `parent` by `step`.
    fn child(&mut self, parent: usize, step: Step) -> usize;
    /// Notes `event` at `node`.
    fn note(&mut self, node: usize, event: Event);
}

/// A watcher that keeps nothing: a plain read pays nothing for the report.
pub(super) struct Blind;

impl Watcher for Blind {
    fn child(&mut self, _parent: usize, _step: Step) -> usize {
        0
    }
    fn note(&mut self, _node: usize, _event: Event) {}
}

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
