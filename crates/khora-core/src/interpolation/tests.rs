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

use super::*;
use crate::math::Vec3;

fn entity(index: u32) -> EntityId {
    EntityId {
        index,
        generation: 0,
    }
}

fn at(x: f32) -> AffineTransform {
    AffineTransform(crate::math::Mat4::from_translation(Vec3::new(x, 0.0, 0.0)))
}

#[test]
fn record_then_read_round_trips() {
    let mut store = TransformInterpolation::new();
    assert!(store.previous(entity(0)).is_none());
    store.record(entity(0), at(3.0));
    assert_eq!(
        store.previous(entity(0)).map(|t| t.translation()),
        Some(Vec3::new(3.0, 0.0, 0.0))
    );
}

#[test]
fn record_overwrites_previous_value() {
    let mut store = TransformInterpolation::new();
    store.record(entity(1), at(1.0));
    store.record(entity(1), at(2.0));
    assert_eq!(
        store.previous(entity(1)).map(|t| t.translation()),
        Some(Vec3::new(2.0, 0.0, 0.0))
    );
    assert_eq!(store.len(), 1);
}

#[test]
fn retain_live_drops_absent_entities() {
    let mut store = TransformInterpolation::new();
    store.record(entity(0), at(0.0));
    store.record(entity(1), at(1.0));
    store.record(entity(2), at(2.0));

    let live: HashSet<EntityId> = [entity(0), entity(2)].into_iter().collect();
    store.retain_live(&live);

    assert!(store.previous(entity(0)).is_some());
    assert!(store.previous(entity(1)).is_none(), "despawned id pruned");
    assert!(store.previous(entity(2)).is_some());
    assert_eq!(store.len(), 2);
}

#[test]
fn empty_live_set_clears_the_store() {
    let mut store = TransformInterpolation::new();
    store.record(entity(0), at(0.0));
    store.retain_live(&HashSet::new());
    assert!(store.is_empty());
}
