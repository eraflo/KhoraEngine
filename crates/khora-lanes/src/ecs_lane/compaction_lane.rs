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

//! A lane for compacting component pages by cleaning up orphaned data.

use khora_data::ecs::{PageIndex, SemanticDomain, WorldMaintenance};

/// A work plan for a single frame's garbage collection pass.
#[derive(Debug, Default)]
pub struct GcWorkPlan {
    /// The budget to determine how many items we can clean this frame.
    pub budget: usize,
    /// The list of orphaned data locations to clean up.
    pub items_to_clean: Vec<(PageIndex, SemanticDomain)>,
    /// The list of pages that have fragmentation (holes) to be vacuumed.
    /// Stores `(page_index, hole_row_index)`.
    pub pages_to_vacuum: Vec<(u32, u32)>,
}

/// The lane responsible for executing the physical cleanup of orphaned component data.
#[derive(Debug, Default)]
pub struct CompactionLane;

impl CompactionLane {
    /// Creates a new `CompactionLane`.
    pub fn new() -> Self {
        Self
    }

    /// Executes the compaction work defined in the `GcWorkPlan`.
    ///
    /// It requires a trait object with `WorldMaintenance` capabilities to perform
    /// its low-level cleanup operations.
    pub fn run(&self, world: &mut dyn WorldMaintenance, work_plan: &GcWorkPlan) {
        // 1. Cleanup orphans
        for (location, domain) in work_plan.items_to_clean.iter().take(work_plan.budget) {
            world.cleanup_orphan_at(*location, *domain);
        }

        // 2. Vacuum pages (compact fragmentation)
        for (page_id, hole_row_index) in work_plan.pages_to_vacuum.iter().take(work_plan.budget) {
            world.vacuum_hole_at(*page_id, *hole_row_index);
        }
    }
}
