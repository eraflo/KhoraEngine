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

//! The Intelligent Subsystem Agent for garbage collection.

use std::collections::VecDeque;

use khora_data::ecs::{PageIndex, SemanticDomain, World};
use khora_lanes::ecs_lane::{CompactionLane, GcWorkPlan};

/// The agent responsible for managing the garbage collection of orphaned component data.
///
/// This agent collects orphaned data locations, decides when and how much to clean
/// based on a strategy, and dispatches the work to a `CompactionLane`.
pub struct GarbageCollectorAgent {
    /// A queue of locations pointing to orphaned data that needs cleanup.
    pending_cleanup: VecDeque<(PageIndex, SemanticDomain)>,
    /// The worker lane that performs the actual data compaction.
    compaction_lane: CompactionLane,
}

impl GarbageCollectorAgent {
    /// Creates a new `GarbageCollectorAgent`.
    pub fn new() -> Self {
        Self {
            pending_cleanup: VecDeque::new(),
            compaction_lane: CompactionLane::new(),
        }
    }

    /// Adds a new orphaned data location to the cleanup queue.
    /// This is called by the `World`'s user after `add/remove_component`.
    pub fn queue_cleanup(&mut self, page_index: PageIndex, domain: SemanticDomain) {
        self.pending_cleanup.push_back((page_index, domain));
    }

    /// Runs the agent's decision-making and execution logic for one frame.
    pub fn run(&mut self, world: &mut World) {
        if self.pending_cleanup.is_empty() {
            return;
        }

        // --- SAA Strategy Logic (currently a simple placeholder) ---
        // In the future, this budget would be determined by the DCC based on system load.
        const MAX_CLEANUP_PER_FRAME: usize = 10;
        let budget = self.pending_cleanup.len().min(MAX_CLEANUP_PER_FRAME);

        // --- Prepare Work Plan ---
        let items_to_clean: Vec<_> = self.pending_cleanup.drain(..budget).collect();
        let work_plan = GcWorkPlan {
            budget,
            items_to_clean,
            pages_to_vacuum: Vec::new(),
        };

        // --- Dispatch to Lane ---
        self.compaction_lane.run(world, &work_plan);
    }
}

impl Default for GarbageCollectorAgent {
    fn default() -> Self {
        Self::new()
    }
}
