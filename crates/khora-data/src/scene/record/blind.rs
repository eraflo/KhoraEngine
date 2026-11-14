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

//! A watcher that keeps nothing of what a read did.

use super::watch::{Event, Step, Watcher};

/// A watcher that keeps nothing: a plain read pays nothing for the report.
pub(super) struct Blind;

impl Watcher for Blind {
    fn child(&mut self, _parent: usize, _step: Step) -> usize {
        0
    }
    fn note(&mut self, _node: usize, _event: Event) {}
}
