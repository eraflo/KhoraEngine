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

//! Per-frame blackboard for cross-agent communication and hot-path synchronization.
//!
//! The `FrameContext` is created once per frame by the windowing driver and
//! inserted into the frame's service registry. Agents use it to:
//! - **Insert/retrieve arbitrary data** via `insert::<T>()` / `get::<T>()`
//! - **Spawn async tasks** on the shared tokio runtime via `spawn()`
//! - **Synchronize** with other agents via type-safe `StageHandle<T>`
//!
//! # Design
//!
//! - **Data** is stored in an [`anymap::AnyMap`] (type-keyed, O(1) lookup).
//! - **Stages** are synchronization primitives — each agent creates its own
//!   `StageHandle<T>` and other agents wait on it via `get::<StageHandle<T>>()`.
//! - **Tasks** spawned via `spawn()` are automatically tracked; the runner
//!   calls `wait_for_all()` after `engine.tick()` to wait for all hot-path work.
//!
//! # Example
//!
//! ```ignore
//! // ShadowAgent
//! fctx.insert_stage::<ShadowDone>();
//! fctx.spawn(async move {
//!     // ... encode shadow passes ...
//!     fctx.get::<StageHandle<ShadowDone>>().mark_done();
//! });
//!
//! // RenderAgent
//! let shadow = fctx.get::<StageHandle<ShadowDone>>().cloned();
//! fctx.spawn(async move {
//!     shadow.unwrap().wait().await;
//!     // ... encode main pass with shadow data ...
//! });
//! ```

use std::marker::PhantomData;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tokio::runtime::Handle;

use crate::utils::any_map::AnyMap;

// ─────────────────────────────────────────────────────────────────────
// StageHandle — type-safe synchronization primitive
// ─────────────────────────────────────────────────────────────────────

/// Type-safe stage handle — shared between agents for cross-frame synchronization.
///
/// Created via [`FrameContext::insert_stage`], stored in the blackboard,
/// and retrieved by type: `ctx.get::<StageHandle<MyStage>>()`.
///
/// The marker type `T` ensures compile-time safety — no strings needed.
pub struct StageHandle<T: 'static> {
    notify: Arc<tokio::sync::Notify>,
    _marker: PhantomData<fn() -> T>,
}

// Safety: StageHandle only contains Arc and PhantomData.
unsafe impl<T: 'static> Send for StageHandle<T> {}
unsafe impl<T: 'static> Sync for StageHandle<T> {}

impl<T: 'static> StageHandle<T> {
    fn new() -> Self {
        Self {
            notify: Arc::new(tokio::sync::Notify::new()),
            _marker: PhantomData,
        }
    }

    /// Signals that this stage is complete. Wakes all waiters.
    pub fn mark_done(&self) {
        self.notify.notify_waiters();
    }

    /// Awaits stage completion. Returns immediately if already done.
    pub async fn wait(&self) {
        self.notify.notified().await;
    }
}

impl<T: 'static> Clone for StageHandle<T> {
    fn clone(&self) -> Self {
        Self {
            notify: Arc::clone(&self.notify),
            _marker: PhantomData,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────
// FrameContext — per-frame blackboard + tokio task tracking
// ─────────────────────────────────────────────────────────────────────

/// Per-frame blackboard for cross-agent communication and hot-path synchronization.
///
/// Created once per frame by the windowing driver, inserted into the frame's
/// service registry, and consumed by the runner after all agents complete.
pub struct FrameContext {
    /// Type-erased blackboard — any agent can insert/retrieve by type.
    /// Wrapped in Arc<Mutex<>> for interior mutability across agents.
    data: Arc<Mutex<AnyMap>>,
    /// Tokio runtime handle for spawning hot-path tasks.
    tokio: Handle,
    /// Count of outstanding hot-path tasks across all agents.
    pending_tasks: Arc<AtomicUsize>,
    /// Signal that all hot-path tasks for this frame are complete.
    all_done: Arc<tokio::sync::Notify>,
}

// Safety: FrameContext uses atomic ops and tokio primitives.
unsafe impl Send for FrameContext {}
unsafe impl Sync for FrameContext {}

impl FrameContext {
    /// Creates a new frame context with the given tokio runtime handle.
    pub fn new(tokio: Handle) -> Self {
        Self {
            data: Arc::new(Mutex::new(AnyMap::new())),
            tokio,
            pending_tasks: Arc::new(AtomicUsize::new(0)),
            all_done: Arc::new(tokio::sync::Notify::new()),
        }
    }

    // ─── Blackboard ─────────────────────────────────────────────

    /// Inserts a value into the blackboard, replacing any existing value of the same type.
    pub fn insert<T: Send + Sync + 'static>(&self, value: T) {
        self.data.lock().unwrap().insert(value);
    }

    /// Retrieves a cloned Arc of a value of the given type, if present.
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.data.lock().unwrap().get::<T>()
    }

    /// Returns true if a value of type `T` is stored.
    pub fn contains<T: Send + Sync + 'static>(&self) -> bool {
        self.data.lock().unwrap().contains::<T>()
    }

    // ─── Stage synchronization ──────────────────────────────────

    /// Creates a new [`StageHandle<T>`] and inserts it into the blackboard.
    ///
    /// The calling agent (or any other) can later retrieve it via
    /// `get::<StageHandle<T>>()` to wait for or signal completion.
    pub fn insert_stage<T: Send + Sync + 'static>(&self) -> StageHandle<T> {
        let handle = StageHandle::<T>::new();
        self.data.lock().unwrap().insert(handle.clone());
        handle
    }

    // ─── Task spawning ──────────────────────────────────────────

    /// Spawns a hot-path async task on the shared tokio runtime.
    ///
    /// The task counter is incremented before spawn and decremented when the
    /// task completes. The runner calls [`wait_for_all`] after `engine.tick()`
    /// to wait for all spawned tasks.
    pub fn spawn<F>(&self, future: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let pending = Arc::clone(&self.pending_tasks);
        let all_done = Arc::clone(&self.all_done);

        pending.fetch_add(1, Ordering::AcqRel);

        self.tokio.spawn(async move {
            future.await;
            if pending.fetch_sub(1, Ordering::AcqRel) == 1 {
                // This was the last task — signal completion.
                all_done.notify_waiters();
            }
        });
    }

    /// Awaits completion of ALL hot-path tasks spawned this frame.
    ///
    /// Called by the runner after `engine.tick()` returns.
    pub async fn wait_for_all(&self) {
        // Fast path: no tasks were spawned
        if self.pending_tasks.load(Ordering::Acquire) == 0 {
            return;
        }
        self.all_done.notified().await;
    }

    /// Returns the tokio runtime handle for direct access if needed.
    pub fn tokio_handle(&self) -> &Handle {
        &self.tokio
    }
}

impl std::fmt::Debug for FrameContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameContext")
            .field("pending_tasks", &self.pending_tasks.load(Ordering::Relaxed))
            .finish()
    }
}
