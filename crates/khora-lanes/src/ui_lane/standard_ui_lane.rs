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

use khora_core::lane::{Lane, LaneContext, LaneError, LaneKind, Slot};
use khora_core::ui::LayoutSystem;
use khora_data::ecs::World;
use std::sync::{Arc, Mutex};

/// Standard implementation of the UI layout lane.
///
/// This lane acts as the bridge between the ECS world (data) and the
/// hardware/library implementation of the layout system (infra).
pub struct StandardUiLane {
    /// The layout system to use for computation.
    layout_system: Arc<Mutex<Box<dyn LayoutSystem>>>,
}

impl StandardUiLane {
    /// Creates a new `StandardUiLane` with the given layout system.
    pub fn new(layout_system: Arc<Mutex<Box<dyn LayoutSystem>>>) -> Self {
        Self { layout_system }
    }
}

impl Lane for StandardUiLane {
    fn strategy_name(&self) -> &'static str {
        "StandardUi"
    }

    fn lane_kind(&self) -> LaneKind {
        LaneKind::Ui
    }

    fn execute(&self, ctx: &mut LaneContext) -> Result<(), LaneError> {
        // 1. Retrieve the World from the context
        let world = ctx
            .get::<Slot<World>>()
            .ok_or_else(|| LaneError::missing("Slot<World>"))?
            .get();

        // 2. Lock and execute the layout system
        if let Ok(mut layout_system) = self.layout_system.lock() {
            layout_system.compute_layouts(world);
            Ok(())
        } else {
            Err(LaneError::ExecutionFailed(
                "Failed to lock layout system".into(),
            ))
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
