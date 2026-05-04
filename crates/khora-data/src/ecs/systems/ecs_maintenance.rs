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

//! ECS maintenance — drains the queued cleanup / vacuum work each frame.
//!
//! Replaces the previous hardcoded `gw.tick_maintenance()` call in the
//! engine tick. The [`EcsMaintenance`] state lives in the
//! [`ServiceRegistry`] (inserted at engine init) so the system can fetch
//! and tick it without any wiring.

use std::sync::{Arc, Mutex};

use khora_core::ServiceRegistry;

use crate::ecs::{DataSystemRegistration, EcsMaintenance, TickPhase, World};

fn ecs_maintenance_system(world: &mut World, services: &ServiceRegistry) {
    let Some(maintenance) = services.get::<Arc<Mutex<EcsMaintenance>>>() else {
        return;
    };
    if let Ok(mut guard) = maintenance.lock() {
        guard.tick(world);
    }
}

inventory::submit! {
    DataSystemRegistration {
        name: "ecs_maintenance",
        phase: TickPhase::Maintenance,
        run: ecs_maintenance_system,
        order_hint: 0,
        runs_after: &[],
    }
}
