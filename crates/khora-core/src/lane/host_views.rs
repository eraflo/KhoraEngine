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

//! `HostViews` — Views the host application hands the lanes, frame after
//! frame.
//!
//! A Flow projects the `World`; some Views come from no `World` at all — the
//! editor's grid toggle, its gizmo lines, its wireframe switch. The host sets
//! them here (an `Arc<HostViews>` in the runtime's resources, inserted by the
//! engine at boot with each overlay's empty View), and the scheduler publishes
//! the latest value of each into the frame's [`LaneBus`] right after the Flows
//! run, before any agent. A View the host never set is never published.
//!
//! The host writes, the bus reads: lanes see the Views like any other, and no
//! lane or agent ever holds the host's state.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Mutex;

use super::LaneBus;

/// A View the host set, able to publish a copy of itself.
trait HostView: Send + Sync {
    fn publish_into(&self, bus: &mut LaneBus);
}

impl<V: Any + Send + Sync + Clone> HostView for V {
    fn publish_into(&self, bus: &mut LaneBus) {
        bus.publish(self.clone());
    }
}

/// The Views a host application sets, published into every frame's
/// [`LaneBus`].
///
/// One value per View type: setting a View again replaces it. Shared as an
/// `Arc<HostViews>` resource; [`set`](Self::set) takes `&self` so the host
/// writes through the shared handle.
#[derive(Default)]
pub struct HostViews {
    views: Mutex<HashMap<TypeId, Box<dyn HostView>>>,
}

impl HostViews {
    /// An empty set: publishes nothing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the View of type `V` every following frame publishes, replacing
    /// the one set before.
    pub fn set<V: Any + Send + Sync + Clone>(&self, view: V) {
        self.views
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(TypeId::of::<V>(), Box::new(view));
    }

    /// Publishes a copy of every View set so far into `bus`.
    pub fn publish_into(&self, bus: &mut LaneBus) {
        let views = self
            .views
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for view in views.values() {
            (**view).publish_into(bus);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    struct Toggle(bool);

    #[derive(Debug, Clone, PartialEq)]
    struct Lines(Vec<u32>);

    /// **What the host set reaches the bus, latest value first.** Each View
    /// type keeps one value; a second `set` replaces it, and every frame's
    /// fresh bus receives the same copy until the host sets it again.
    #[test]
    fn host_views_reach_every_frame_bus() {
        let views = HostViews::new();
        views.set(Toggle(true));
        views.set(Lines(vec![1, 2]));
        views.set(Toggle(false));

        for frame in 0..2 {
            let mut bus = LaneBus::new();
            views.publish_into(&mut bus);
            assert_eq!(bus.get::<Toggle>(), Some(&Toggle(false)), "frame {frame}");
            assert_eq!(
                bus.get::<Lines>(),
                Some(&Lines(vec![1, 2])),
                "frame {frame}"
            );
        }
    }

    /// A View the host never set is not published: an empty set leaves the
    /// bus empty.
    #[test]
    fn an_empty_host_view_set_publishes_nothing() {
        let views = HostViews::new();
        let mut bus = LaneBus::new();
        views.publish_into(&mut bus);
        assert!(bus.is_empty(), "nothing was set, nothing is published");
    }
}
