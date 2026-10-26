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

//! `LaneBus` — typed read-only bus of [`Flow`] outputs, scoped to one tick.
//!
//! `Flow`s publish typed `View`s into the bus during the Substrate Pass; lanes
//! consume them during the CLAD descent. Outside of lane execution the bus
//! does not exist (it is constructed by the scheduler at frame start and
//! dropped at frame end).
//!
//! # Visibility contract
//!
//! - [`LaneBus::publish`] is public so the `Flow` runner in `khora-control`
//!   can call it, but only Flows publish: lanes never receive a `&mut LaneBus`
//!   (the bus is frozen behind an `Arc` before agents run).
//! - [`LaneBus::get`] is what agents use to hand a View to their lane.
//!
//! [`Flow`]: ../../../khora_data/flow/index.html

use std::any::{Any, TypeId};
use std::collections::HashMap;

use super::io::FrameId;

/// Read-only typed bus carrying [`Flow`] outputs to lanes for one tick.
///
/// An agent reads a view with [`LaneBus::get`] and puts it into its lane's
/// [`LaneContext`](super::LaneContext). The bus itself is constructed by
/// the scheduler at the start of each frame and dropped at the end — its
/// lifetime is strictly tick-scoped.
///
/// [`Flow`]: ../../../khora_data/flow/index.html
pub struct LaneBus {
    views: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
    /// The frame each view was published in.
    stamps: HashMap<TypeId, FrameId>,
    /// The frame views are stamped with now.
    frame: FrameId,
}

impl LaneBus {
    /// Creates an empty bus.
    pub fn new() -> Self {
        Self {
            views: HashMap::new(),
            stamps: HashMap::new(),
            frame: FrameId::default(),
        }
    }

    /// Publishes a typed view into the bus. Replaces any existing entry of
    /// the same type. Used by `Flow` execution; not callable from lane code.
    pub fn publish<V: Any + Send + Sync>(&mut self, view: V) {
        self.views.insert(TypeId::of::<V>(), Box::new(view));
        self.stamps.insert(TypeId::of::<V>(), self.frame);
    }

    /// Returns a shared reference to a view by type, or `None` if no `Flow`
    /// has published one this tick.
    pub fn get<V: Any + Send + Sync>(&self) -> Option<&V> {
        self.views
            .get(&TypeId::of::<V>())
            .and_then(|b| b.downcast_ref::<V>())
    }

    /// Reports whether a view of the given type is present.
    pub fn contains<V: Any + Send + Sync>(&self) -> bool {
        self.views.contains_key(&TypeId::of::<V>())
    }

    /// Number of views currently published.
    pub fn len(&self) -> usize {
        self.views.len()
    }

    /// Whether no views are published.
    pub fn is_empty(&self) -> bool {
        self.views.is_empty()
    }

    /// Clears all published views. Called by the scheduler between ticks
    /// when a bus instance is reused rather than reallocated.
    pub fn clear(&mut self) {
        self.views.clear();
        self.stamps.clear();
    }

    /// Starts `frame`: every view published from now on is stamped with it.
    /// The views already published keep the stamp of the frame they were
    /// published in.
    pub fn set_frame(&mut self, frame: FrameId) {
        self.frame = frame;
    }

    /// The frame views are stamped with now.
    pub fn frame(&self) -> FrameId {
        self.frame
    }

    /// The frame the view of type `V` was published in, or `None` if none was.
    pub fn stamp_of<V: Any + Send + Sync>(&self) -> Option<FrameId> {
        self.stamps.get(&TypeId::of::<V>()).copied()
    }
}

impl Default for LaneBus {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for LaneBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LaneBus")
            .field("views", &self.views.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct TestView {
        value: u32,
    }

    #[test]
    fn publish_then_get_returns_same_value() {
        let mut bus = LaneBus::new();
        bus.publish(TestView { value: 42 });
        assert_eq!(bus.get::<TestView>(), Some(&TestView { value: 42 }));
    }

    #[test]
    fn a_view_keeps_the_stamp_of_the_frame_it_was_published_in() {
        let mut bus = LaneBus::new();
        bus.set_frame(FrameId(4));
        bus.publish(TestView { value: 1 });
        bus.set_frame(FrameId(5));
        assert_eq!(bus.frame(), FrameId(5));
        assert_eq!(bus.stamp_of::<TestView>(), Some(FrameId(4)));
        bus.clear();
        assert_eq!(
            bus.stamp_of::<TestView>(),
            None,
            "clearing drops the stamps"
        );
    }

    #[test]
    fn get_missing_returns_none() {
        let bus = LaneBus::new();
        assert!(bus.get::<TestView>().is_none());
    }

    #[test]
    fn publish_replaces_previous_entry() {
        let mut bus = LaneBus::new();
        bus.publish(TestView { value: 1 });
        bus.publish(TestView { value: 2 });
        assert_eq!(bus.get::<TestView>(), Some(&TestView { value: 2 }));
    }

    #[test]
    fn contains_reports_presence() {
        let mut bus = LaneBus::new();
        assert!(!bus.contains::<TestView>());
        bus.publish(TestView { value: 0 });
        assert!(bus.contains::<TestView>());
    }

    #[test]
    fn clear_removes_all_views() {
        let mut bus = LaneBus::new();
        bus.publish(TestView { value: 1 });
        bus.publish(0u8);
        bus.clear();
        assert!(bus.is_empty());
    }
}
