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

//! ECS maintenance subsystem — garbage collection and page compaction.
//!
//! This module provides a direct maintenance service for the ECS World.
//! Unlike Agents, there is no strategy negotiation — the maintenance runs
//! a fixed number of items per frame to keep frame times predictable.
//!
//! # Usage
//!
//! ```rust,ignore
//! use khora_data::ecs::{World, EcsMaintenance};
//!
//! let mut world = World::new();
//! let mut maintenance = EcsMaintenance::new();
//!
//! // When orphaned data is detected:
//! maintenance.queue_cleanup(page_index, domain);
//!
//! // Each frame:
//! maintenance.tick(&mut world);
//! ```

use std::collections::VecDeque;

use super::{PageIndex, SemanticDomain, World, WorldMaintenance};

const DEFAULT_MAX_PER_FRAME: usize = 10;

/// Direct ECS maintenance service.
///
/// Tracks pending cleanup and vacuum requests and processes them each frame
/// with a configurable budget. This replaces the former `GarbageCollectorAgent`.
pub struct EcsMaintenance {
    pending_cleanup: VecDeque<(PageIndex, SemanticDomain)>,
    pending_vacuum: VecDeque<(u32, u32)>,
    max_per_frame: usize,
    last_cleanup_count: usize,
}

impl EcsMaintenance {
    /// Creates a new maintenance service with default settings.
    pub fn new() -> Self {
        Self {
            pending_cleanup: VecDeque::new(),
            pending_vacuum: VecDeque::new(),
            max_per_frame: DEFAULT_MAX_PER_FRAME,
            last_cleanup_count: 0,
        }
    }

    /// Creates a new maintenance service with a custom per-frame budget.
    pub fn with_budget(max_per_frame: usize) -> Self {
        Self {
            max_per_frame,
            ..Self::new()
        }
    }

    /// Queues a cleanup request for an orphaned data location.
    pub fn queue_cleanup(&mut self, page_index: PageIndex, domain: SemanticDomain) {
        self.pending_cleanup.push_back((page_index, domain));
    }

    /// Queues a vacuum request for a page hole.
    pub fn queue_vacuum(&mut self, page_index: u32, hole_row_index: u32) {
        self.pending_vacuum.push_back((page_index, hole_row_index));
    }

    /// Runs one frame of maintenance on the given world.
    ///
    /// Drains up to `max_per_frame` items from the cleanup and vacuum queues
    /// and executes the compaction operations.
    pub fn tick(&mut self, world: &mut World) {
        if self.pending_cleanup.is_empty() && self.pending_vacuum.is_empty() {
            self.last_cleanup_count = 0;
            return;
        }

        let budget = self.max_per_frame;
        let mut count = 0;

        // Cleanup orphans
        let cleanup_count = budget.min(self.pending_cleanup.len());
        for _ in 0..cleanup_count {
            if let Some((location, domain)) = self.pending_cleanup.pop_front() {
                world.cleanup_orphan_at(location, domain);
                count += 1;
            }
        }

        // Vacuum pages (compact fragmentation)
        let vacuum_budget = (budget.saturating_sub(cleanup_count)).min(self.pending_vacuum.len());
        for _ in 0..vacuum_budget {
            if let Some((page_id, hole_row)) = self.pending_vacuum.pop_front() {
                world.vacuum_hole_at(page_id, hole_row);
                count += 1;
            }
        }

        self.last_cleanup_count = count;

        if count > 0 {
            log::trace!(
                "EcsMaintenance: Cleaned {} items (cleanup_queue={}, vacuum_queue={})",
                count,
                self.pending_cleanup.len(),
                self.pending_vacuum.len(),
            );
        }
    }

    /// Returns the total number of pending requests.
    pub fn pending_count(&self) -> usize {
        self.pending_cleanup.len() + self.pending_vacuum.len()
    }

    /// Returns the number of items cleaned in the last tick.
    pub fn last_cleanup_count(&self) -> usize {
        self.last_cleanup_count
    }

    /// Returns the maximum items processed per frame.
    pub fn max_per_frame(&self) -> usize {
        self.max_per_frame
    }
}

impl Default for EcsMaintenance {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_maintenance_empty_tick() {
        let mut world = World::new();
        let mut maintenance = EcsMaintenance::new();
        maintenance.tick(&mut world);
        assert_eq!(maintenance.last_cleanup_count(), 0);
    }

    #[test]
    fn test_maintenance_budget() {
        let maintenance = EcsMaintenance::with_budget(20);
        assert_eq!(maintenance.max_per_frame(), 20);
    }

    #[test]
    fn test_maintenance_queue_count() {
        let mut maintenance = EcsMaintenance::new();
        maintenance.queue_cleanup(
            PageIndex {
                page_id: 0,
                row_index: 0,
            },
            SemanticDomain::Render,
        );
        maintenance.queue_cleanup(
            PageIndex {
                page_id: 1,
                row_index: 0,
            },
            SemanticDomain::Physics,
        );
        maintenance.queue_vacuum(0, 5);
        assert_eq!(maintenance.pending_count(), 3);
    }
}
