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

//! Auto-registration of [`Flow`](super::Flow) implementations via `inventory`.

use khora_core::lane::LaneBus;
use khora_core::Runtime;

use crate::ecs::{SemanticDomain, World};

/// Registration entry for a [`Flow`](super::Flow) — submitted by each
/// concrete Flow implementation via [`inventory::submit!`].
///
/// Type erasure: because [`Flow`](super::Flow) has an associated `View`
/// type, we cannot store trait objects directly. Each Flow provides a
/// trampoline `run` function that owns its lifecycle (typically delegating
/// to a `static OnceLock<Mutex<MyFlow>>` instance) and publishes its View
/// into the [`LaneBus`].
pub struct FlowRegistration {
    /// Stable identifier — matches `Flow::NAME`.
    pub name: &'static str,
    /// Domain this Flow serves — matches `Flow::DOMAIN`.
    pub domain: SemanticDomain,
    /// Trampoline that runs select + project and publishes the View.
    pub run: fn(&mut World, &mut LaneBus, &Runtime),
}

inventory::collect!(FlowRegistration);

/// One Substrate-Pass execution of `flow`, with view-cache support. This is
/// the body of the [`register_flow!`] trampoline, extracted so the caching
/// behaviour is a single, directly testable code path.
///
/// `cache` holds the `(key, view)` pair of the previously published view.
/// When [`Flow::cache_key`] returns `Some(k)` equal to the cached key, the
/// cached view is republished (cloned) without re-running
/// `select`/`project`; otherwise the flow projects normally and the result
/// replaces the cache (only when caching is enabled — a `None` key clears
/// it, so a flow can opt out dynamically without risking a stale entry).
pub fn run_flow_cached<F: super::Flow>(
    flow: &mut F,
    cache: &mut Option<(u64, F::View)>,
    world: &mut World,
    bus: &mut LaneBus,
    runtime: &Runtime,
) {
    let key = flow.cache_key(world, runtime);
    if let (Some(k), Some((cached_key, cached_view))) = (key, cache.as_ref()) {
        if k == *cached_key {
            bus.publish(cached_view.clone());
            return;
        }
    }

    let sel = flow.select(world, runtime);
    let view = flow.project(world, &sel, runtime);
    *cache = key.map(|k| (k, view.clone()));
    bus.publish(view);
}

/// Convenience macro for declaring a Flow registration with the standard
/// trampoline (single static instance, no per-frame allocation).
///
/// # Example
///
/// ```rust,ignore
/// use khora_data::flow::register_flow;
///
/// pub struct MyFlow { /* ... */ }
/// impl khora_data::flow::Flow for MyFlow { /* ... */ }
///
/// register_flow!(MyFlow);
/// ```
#[macro_export]
macro_rules! register_flow {
    ($flow_ty:ty) => {
        const _: () = {
            fn run_flow(
                world: &mut $crate::ecs::World,
                bus: &mut khora_core::lane::LaneBus,
                runtime: &khora_core::Runtime,
            ) {
                use std::sync::{Mutex, OnceLock};
                // The flow instance and its view cache live together behind
                // the same lock: the cached `(key, view)` of the previously
                // published view, reused when `Flow::cache_key` matches.
                type FlowState = (
                    $flow_ty,
                    Option<(u64, <$flow_ty as $crate::flow::Flow>::View)>,
                );
                static INSTANCE: OnceLock<Mutex<FlowState>> = OnceLock::new();
                let mut state = INSTANCE
                    .get_or_init(|| Mutex::new((<$flow_ty as Default>::default(), None)))
                    .lock()
                    .expect("Flow mutex poisoned");
                let (flow, cache) = &mut *state;
                $crate::flow::run_flow_cached(flow, cache, world, bus, runtime);
            }
            inventory::submit! {
                $crate::flow::FlowRegistration {
                    name: <$flow_ty as $crate::flow::Flow>::NAME,
                    domain: <$flow_ty as $crate::flow::Flow>::DOMAIN,
                    run: run_flow,
                }
            }
        };
    };
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use khora_core::lane::LaneBus;
    use khora_core::Runtime;

    use crate::ecs::{AudioListener, AudioSource, SemanticDomain, World};
    use crate::flow::{combine_cache_key, AudioFlow, AudioView, Flow, Selection};

    /// Minimal view carrying the projection count of the flow that built it.
    #[derive(Debug, Clone, PartialEq)]
    struct CountedView {
        projections: usize,
    }

    /// Cached test flow keyed on the Audio + Spatial epochs, mirroring the
    /// real `AudioFlow` key; counts how many times `project` actually runs.
    #[derive(Default)]
    struct CountedFlow {
        projections: AtomicUsize,
    }

    impl Flow for CountedFlow {
        type View = CountedView;

        const DOMAIN: SemanticDomain = SemanticDomain::Audio;
        const NAME: &'static str = "counted_test";

        fn cache_key(&self, world: &World, _runtime: &Runtime) -> Option<u64> {
            Some(combine_cache_key([
                world.instance_id(),
                world.domain_epoch(SemanticDomain::Audio),
                world.domain_epoch(SemanticDomain::Spatial),
            ]))
        }

        fn project(&self, _world: &World, _sel: &Selection, _runtime: &Runtime) -> CountedView {
            CountedView {
                projections: self.projections.fetch_add(1, Ordering::Relaxed) + 1,
            }
        }
    }

    /// Flow that keeps the default `cache_key` (`None`) — never cached.
    #[derive(Default)]
    struct UncachedFlow {
        projections: AtomicUsize,
    }

    impl Flow for UncachedFlow {
        type View = CountedView;

        const DOMAIN: SemanticDomain = SemanticDomain::Audio;
        const NAME: &'static str = "uncached_test";

        fn project(&self, _world: &World, _sel: &Selection, _runtime: &Runtime) -> CountedView {
            CountedView {
                projections: self.projections.fetch_add(1, Ordering::Relaxed) + 1,
            }
        }
    }

    #[test]
    fn cached_flow_republishes_without_reprojecting_when_world_is_unchanged() {
        let mut world = World::new();
        let runtime = Runtime::new();
        let mut flow = CountedFlow::default();
        let mut cache = None;

        let mut bus = LaneBus::new();
        super::run_flow_cached(&mut flow, &mut cache, &mut world, &mut bus, &runtime);
        assert_eq!(flow.projections.load(Ordering::Relaxed), 1);
        assert_eq!(bus.get::<CountedView>(), Some(&CountedView { projections: 1 }));

        // Nothing mutated the world: the second run must republish the
        // cached view instead of projecting again.
        let mut bus = LaneBus::new();
        super::run_flow_cached(&mut flow, &mut cache, &mut world, &mut bus, &runtime);
        assert_eq!(
            flow.projections.load(Ordering::Relaxed),
            1,
            "unchanged world must not re-run project"
        );
        assert_eq!(
            bus.get::<CountedView>(),
            Some(&CountedView { projections: 1 }),
            "the cached view must still be published into the fresh bus"
        );
    }

    #[test]
    fn cached_flow_reprojects_after_a_domain_mutation() {
        let mut world = World::new();
        let runtime = Runtime::new();
        let mut flow = CountedFlow::default();
        let mut cache = None;

        let mut bus = LaneBus::new();
        super::run_flow_cached(&mut flow, &mut cache, &mut world, &mut bus, &runtime);
        assert_eq!(flow.projections.load(Ordering::Relaxed), 1);

        // Mutate the Audio domain — the key changes, the cache must miss.
        world.spawn(AudioListener);

        let mut bus = LaneBus::new();
        super::run_flow_cached(&mut flow, &mut cache, &mut world, &mut bus, &runtime);
        assert_eq!(
            flow.projections.load(Ordering::Relaxed),
            2,
            "a mutated domain must invalidate the cached view"
        );
        assert_eq!(bus.get::<CountedView>(), Some(&CountedView { projections: 2 }));
    }

    #[test]
    fn uncached_flow_reprojects_every_run() {
        let mut world = World::new();
        let runtime = Runtime::new();
        let mut flow = UncachedFlow::default();
        let mut cache = None;

        for expected in 1..=3 {
            let mut bus = LaneBus::new();
            super::run_flow_cached(&mut flow, &mut cache, &mut world, &mut bus, &runtime);
            assert_eq!(flow.projections.load(Ordering::Relaxed), expected);
            assert!(cache.is_none(), "a `None` key must never populate the cache");
        }
    }

    #[test]
    fn audio_flow_is_never_stale_after_an_audio_source_insert() {
        let mut world = World::new();
        let runtime = Runtime::new();
        let mut flow = AudioFlow;
        let mut cache = None;

        // Prime the cache with an empty scene.
        let mut bus = LaneBus::new();
        super::run_flow_cached(&mut flow, &mut cache, &mut world, &mut bus, &runtime);
        let view = bus.get::<AudioView>().expect("AudioFlow publishes a view");
        assert_eq!(view.source_count, 0);

        // Insert an AudioSource: the next run must re-project, not serve
        // the cached (now outdated) view.
        world.spawn(AudioSource::default());

        let mut bus = LaneBus::new();
        super::run_flow_cached(&mut flow, &mut cache, &mut world, &mut bus, &runtime);
        let view = bus.get::<AudioView>().expect("AudioFlow publishes a view");
        assert_eq!(
            view.source_count, 1,
            "the published view must reflect the inserted AudioSource"
        );
    }
}
