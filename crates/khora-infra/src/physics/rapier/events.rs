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

use rapier3d::prelude::*;
use std::sync::{Arc, Mutex};

pub struct RapierEventHandler {
    pub events: Arc<Mutex<Vec<khora_core::physics::CollisionEvent>>>,
}

impl EventHandler for RapierEventHandler {
    fn handle_collision_event(
        &self,
        _bodies: &RigidBodySet,
        _colliders: &ColliderSet,
        event: CollisionEvent,
        _contact_pair: Option<&ContactPair>,
    ) {
        let mut events = self.events.lock().unwrap();
        match event {
            CollisionEvent::Started(h1, h2, _) => {
                events.push(khora_core::physics::CollisionEvent::Started(
                    khora_core::physics::ColliderHandle(h1.into_raw_parts().0 as u64),
                    khora_core::physics::ColliderHandle(h2.into_raw_parts().0 as u64),
                ));
            }
            CollisionEvent::Stopped(h1, h2, _) => {
                events.push(khora_core::physics::CollisionEvent::Stopped(
                    khora_core::physics::ColliderHandle(h1.into_raw_parts().0 as u64),
                    khora_core::physics::ColliderHandle(h2.into_raw_parts().0 as u64),
                ));
            }
        }
    }

    fn handle_contact_force_event(
        &self,
        _dt: Real,
        _bodies: &RigidBodySet,
        _colliders: &ColliderSet,
        _contact_pair: &ContactPair,
        _total_force_magnitude: f32,
    ) {
    }
}
