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

use crate::ecs::{ComponentKey, Role, SemanticDomain, World};
use crate::flow::FlowContext;

/// A [`Flow`](super::Flow)'s registration, submitted by
/// [`register_flow!`](crate::register_flow).
///
/// Type-erased, since a Flow has an associated `View`: its `run` function owns
/// the Flow's instance and publishes the View into the [`LaneBus`].
pub struct FlowRegistration {
    /// Stable identifier — matches `Flow::NAME`.
    pub name: &'static str,
    /// Domain this Flow serves — matches `Flow::DOMAIN`.
    pub domain: SemanticDomain,
    /// Whose work it projects for — matches `Flow::ROLE`.
    pub role: Role,
    /// Trampoline that runs select + project and publishes the View.
    pub run: fn(&mut World, &mut LaneBus, &Runtime, FlowContext),
    /// Components this Flow watches for change, by key.
    ///
    /// The `World` records each as observed ([`World::observe`]); the runner
    /// keeps the Flow's last-seen tick.
    pub observes: &'static [fn() -> ComponentKey],
}

inventory::collect!(FlowRegistration);

/// Runs `flow` once, republishing its cached view while its key is unchanged.
///
/// The body of the [`register_flow!`](crate::register_flow) trampoline. `cache` holds the last
/// published `(key, view)`, keyed by [`Flow::cache_key`](super::Flow::cache_key);
/// a `None` key clears it, so a stale view is never served.
pub fn run_flow_cached<F: super::Flow>(
    flow: &mut F,
    cache: &mut Option<(u64, F::View)>,
    world: &mut World,
    bus: &mut LaneBus,
    runtime: &Runtime,
    ctx: FlowContext,
) {
    // A view projected within one scope is not the view of another, nor of
    // the same scope once the marker moved: the scope and the entities it
    // admits are part of every key. The marker may live in any domain, so no
    // epoch the Flow folds is sure to move with it.
    let key = flow.cache_key(world, runtime).map(|key| match ctx.scope {
        None => key,
        Some(scope) => {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            scope.hash(&mut hasher);
            let mut admitted = world.holders(scope);
            admitted.sort_by_key(|entity| (entity.index, entity.generation));
            for entity in admitted {
                (entity.index, entity.generation).hash(&mut hasher);
            }
            super::combine_cache_key([key, hasher.finish()])
        }
    });
    if let (Some(k), Some((cached_key, cached_view))) = (key, cache.as_ref()) {
        if k == *cached_key {
            bus.publish(cached_view.clone());
            return;
        }
    }

    let sel = flow.select(world, runtime, ctx);
    let view = flow.project(world, &sel, runtime);
    *cache = key.map(|k| (k, view.clone()));
    bus.publish(view);
}

/// Registers a Flow with the standard trampoline: one static instance and its
/// view cache.
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
/// // A Flow watching components for change names their types:
/// register_flow!(MyWatchingFlow, observes = [UiText]);
/// ```
#[macro_export]
macro_rules! register_flow {
    ($flow_ty:ty) => {
        $crate::register_flow!($flow_ty, observes = []);
    };
    ($flow_ty:ty, observes = [$($observed:ty),* $(,)?]) => {
        const _: () = {
            fn run_flow(
                world: &mut $crate::ecs::World,
                bus: &mut khora_core::lane::LaneBus,
                runtime: &khora_core::Runtime,
                ctx: $crate::flow::FlowContext,
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
                $crate::flow::run_flow_cached(flow, cache, world, bus, runtime, ctx);
            }
            inventory::submit! {
                $crate::flow::FlowRegistration {
                    name: <$flow_ty as $crate::flow::Flow>::NAME,
                    domain: <$flow_ty as $crate::flow::Flow>::DOMAIN,
                    role: <$flow_ty as $crate::flow::Flow>::ROLE,
                    run: run_flow,
                    observes: &[$($crate::ecs::ComponentKey::of::<$observed>),*],
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

    use crate::ecs::{AudioListener, AudioSource, Role, SemanticDomain, World};
    use crate::flow::{combine_cache_key, AudioFlow, AudioView, Flow, FlowContext, Selection};

    /// Minimal view carrying the projection count of the flow that built it.
    #[derive(Debug, Clone, PartialEq)]
    struct CountedView {
        projections: usize,
    }

    /// A cached test flow keyed like `AudioFlow`, on the Audio and Spatial
    /// epochs.
    ///
    /// Counts how many times `project` runs.
    #[derive(Default)]
    struct CountedFlow {
        projections: AtomicUsize,
    }

    impl Flow for CountedFlow {
        type View = CountedView;

        const DOMAIN: SemanticDomain = SemanticDomain::Audio;
        const NAME: &'static str = "counted_test";
        const ROLE: Role = Role::Upkeep;

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
        const ROLE: Role = Role::Upkeep;

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
        super::run_flow_cached(
            &mut flow,
            &mut cache,
            &mut world,
            &mut bus,
            &runtime,
            FlowContext::default(),
        );
        assert_eq!(flow.projections.load(Ordering::Relaxed), 1);
        assert_eq!(
            bus.get::<CountedView>(),
            Some(&CountedView { projections: 1 })
        );

        // Nothing mutated the world: the second run must republish the
        // cached view instead of projecting again.
        let mut bus = LaneBus::new();
        super::run_flow_cached(
            &mut flow,
            &mut cache,
            &mut world,
            &mut bus,
            &runtime,
            FlowContext::default(),
        );
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
        super::run_flow_cached(
            &mut flow,
            &mut cache,
            &mut world,
            &mut bus,
            &runtime,
            FlowContext::default(),
        );
        assert_eq!(flow.projections.load(Ordering::Relaxed), 1);

        // Mutate the Audio domain — the key changes, the cache must miss.
        world.spawn(AudioListener);

        let mut bus = LaneBus::new();
        super::run_flow_cached(
            &mut flow,
            &mut cache,
            &mut world,
            &mut bus,
            &runtime,
            FlowContext::default(),
        );
        assert_eq!(
            flow.projections.load(Ordering::Relaxed),
            2,
            "a mutated domain must invalidate the cached view"
        );
        assert_eq!(
            bus.get::<CountedView>(),
            Some(&CountedView { projections: 2 })
        );
    }

    /// Handing out `&mut` access is not a write.
    ///
    /// A cached flow keyed on the domain epochs keeps its view while its
    /// components are only read through `get_mut` and `query_mut`, and
    /// re-projects once one is written.
    #[test]
    fn cached_flow_keeps_its_view_when_access_is_handed_out_but_nothing_is_written() {
        let mut world = World::new();
        let runtime = Runtime::new();
        let mut flow = CountedFlow::default();
        let mut cache = None;
        let listener = world.spawn(AudioListener);
        world.spawn(AudioSource::default());

        let mut bus = LaneBus::new();
        super::run_flow_cached(
            &mut flow,
            &mut cache,
            &mut world,
            &mut bus,
            &runtime,
            FlowContext::default(),
        );
        assert_eq!(flow.projections.load(Ordering::Relaxed), 1);
        // The clock moves past the setup, as it does between runs.
        world.end_run();

        assert!(world.get_mut::<AudioListener>(listener).is_some());
        let sources = world.query_mut::<&mut AudioSource>().count();
        assert_eq!(sources, 1);

        let mut bus = LaneBus::new();
        super::run_flow_cached(
            &mut flow,
            &mut cache,
            &mut world,
            &mut bus,
            &runtime,
            FlowContext::default(),
        );
        assert_eq!(
            flow.projections.load(Ordering::Relaxed),
            1,
            "access handed out with no write must not invalidate the cached view"
        );

        *world
            .get_mut::<AudioListener>(listener)
            .expect("the listener is there") = AudioListener;

        let mut bus = LaneBus::new();
        super::run_flow_cached(
            &mut flow,
            &mut cache,
            &mut world,
            &mut bus,
            &runtime,
            FlowContext::default(),
        );
        assert_eq!(
            flow.projections.load(Ordering::Relaxed),
            2,
            "a write of the flow's domain must invalidate the cached view"
        );
    }

    #[test]
    fn uncached_flow_reprojects_every_run() {
        let mut world = World::new();
        let runtime = Runtime::new();
        let mut flow = UncachedFlow::default();
        let mut cache = None;

        for expected in 1..=3 {
            let mut bus = LaneBus::new();
            super::run_flow_cached(
                &mut flow,
                &mut cache,
                &mut world,
                &mut bus,
                &runtime,
                FlowContext::default(),
            );
            assert_eq!(flow.projections.load(Ordering::Relaxed), expected);
            assert!(
                cache.is_none(),
                "a `None` key must never populate the cache"
            );
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
        super::run_flow_cached(
            &mut flow,
            &mut cache,
            &mut world,
            &mut bus,
            &runtime,
            FlowContext::default(),
        );
        let view = bus.get::<AudioView>().expect("AudioFlow publishes a view");
        assert_eq!(view.source_count, 0);

        // Insert an AudioSource: the next run must re-project, not serve
        // the cached (now outdated) view.
        world.spawn(AudioSource::default());

        let mut bus = LaneBus::new();
        super::run_flow_cached(
            &mut flow,
            &mut cache,
            &mut world,
            &mut bus,
            &runtime,
            FlowContext::default(),
        );
        let view = bus.get::<AudioView>().expect("AudioFlow publishes a view");
        assert_eq!(
            view.source_count, 1,
            "the published view must reflect the inserted AudioSource"
        );
    }
}
