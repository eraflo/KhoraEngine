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

//! ECS maintenance: compacts the pages migrations left orphaned rows in, a few per frame.
//!
//! See the book: concepts/ecs.md, "Maintenance is a service, not an agent".
//!
//! # Usage
//!
//! ```rust,ignore
//! use khora_data::ecs::{World, EcsMaintenance};
//!
//! let mut world = World::new();
//! let mut maintenance = EcsMaintenance::new();
//!
//! // Each frame:
//! maintenance.tick(&mut world);
//! ```

use super::World;

const DEFAULT_MAX_PER_FRAME: usize = 10;

/// The ECS maintenance service: compacts up to `max_per_frame` dirty pages each frame.
pub struct EcsMaintenance {
    max_per_frame: usize,
    last_compacted_count: usize,
}

impl EcsMaintenance {
    /// Creates a new maintenance service with the default per-frame budget.
    pub fn new() -> Self {
        Self {
            max_per_frame: DEFAULT_MAX_PER_FRAME,
            last_compacted_count: 0,
        }
    }

    /// Creates a new maintenance service with a custom per-frame page budget.
    pub fn with_budget(max_per_frame: usize) -> Self {
        Self {
            max_per_frame,
            last_compacted_count: 0,
        }
    }

    /// Compacts up to `max_per_frame` dirty pages.
    ///
    /// Pages beyond the budget wait for the next frame; queries skip orphan rows meanwhile.
    pub fn tick(&mut self, world: &mut World) {
        self.last_compacted_count = world.run_compaction(self.max_per_frame);

        if self.last_compacted_count > 0 {
            log::trace!(
                "EcsMaintenance: compacted {} page(s)",
                self.last_compacted_count
            );
        }
    }

    /// Returns the number of pages compacted in the last [`tick`](Self::tick).
    pub fn last_compacted_count(&self) -> usize {
        self.last_compacted_count
    }

    /// Returns the maximum number of pages compacted per frame.
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
    fn empty_tick_compacts_nothing() {
        let mut world = World::new();
        let mut maintenance = EcsMaintenance::new();
        maintenance.tick(&mut world);
        assert_eq!(maintenance.last_compacted_count(), 0);
    }

    #[test]
    fn budget_is_configurable() {
        let maintenance = EcsMaintenance::with_budget(20);
        assert_eq!(maintenance.max_per_frame(), 20);
    }

    #[test]
    fn default_budget() {
        assert_eq!(EcsMaintenance::new().max_per_frame(), DEFAULT_MAX_PER_FRAME);
    }
}
