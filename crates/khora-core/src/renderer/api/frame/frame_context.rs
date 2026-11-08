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

//! Per-frame blackboard for cross-agent communication.
//!
//! The `FrameContext` is created once by the windowing driver and inserted
//! into the runtime's resources. Agents and lanes that declare it read and
//! write arbitrary data by type: `insert::<T>()`, `get::<T>()`. The frame's
//! render target is published there, for instance.
//!
//! Data is stored in an [`AnyMap`] (type-keyed, O(1) lookup) behind a lock,
//! so the context is shared as `Arc<FrameContext>` and reused across frames.
//! Ordering between agents is the scheduler's business
//! ([`AgentCompletionMap`](crate::agent::AgentCompletionMap)), not this
//! blackboard's.

use std::sync::{Arc, Mutex};

use crate::util::any_map::AnyMap;

/// Per-frame blackboard for cross-agent communication.
///
/// Created once by the windowing driver and shared through the runtime's
/// resources; any agent can insert or retrieve a value by its type.
pub struct FrameContext {
    /// Type-erased blackboard, behind a lock for interior mutability across
    /// agents.
    data: Arc<Mutex<AnyMap>>,
}

impl FrameContext {
    /// Creates an empty frame context.
    pub fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(AnyMap::new())),
        }
    }

    /// Inserts a value into the blackboard, replacing any existing value of the same type.
    pub fn insert<T: Send + Sync + 'static>(&self, value: T) {
        self.data
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(value);
    }

    /// Retrieves a cloned Arc of a value of the given type, if present.
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.data
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get::<T>()
    }

    /// Removes the value of type `T`, returning it if one was stored.
    pub fn remove<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.data
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove::<T>()
    }

    /// Returns true if a value of type `T` is stored.
    pub fn contains<T: Send + Sync + 'static>(&self) -> bool {
        self.data
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains::<T>()
    }
}

impl Default for FrameContext {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for FrameContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameContext").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct Exposure(f32);

    #[derive(Debug, PartialEq)]
    struct FrameLabel(&'static str);

    const fn assert_send_sync<T: Send + Sync>() {}
    const _: () = assert_send_sync::<FrameContext>();

    #[test]
    fn frame_context_is_built_without_a_runtime() {
        // No tokio runtime exists on this thread: the blackboard needs none.
        let fctx = FrameContext::new();

        assert!(!fctx.contains::<Exposure>());
        assert!(fctx.get::<Exposure>().is_none());

        fctx.insert(Exposure(1.5));
        fctx.insert(FrameLabel("main"));
        assert!(fctx.contains::<Exposure>());
        assert_eq!(fctx.get::<Exposure>().as_deref(), Some(&Exposure(1.5)));
        assert_eq!(
            fctx.get::<FrameLabel>().as_deref(),
            Some(&FrameLabel("main"))
        );

        // A second value of the same type replaces the first; other types are untouched.
        fctx.insert(Exposure(0.25));
        assert_eq!(fctx.get::<Exposure>().as_deref(), Some(&Exposure(0.25)));
        assert_eq!(
            fctx.get::<FrameLabel>().as_deref(),
            Some(&FrameLabel("main"))
        );

        assert_eq!(fctx.remove::<Exposure>().as_deref(), Some(&Exposure(0.25)));
        assert!(!fctx.contains::<Exposure>());
        assert!(fctx.get::<Exposure>().is_none());
        assert!(fctx.remove::<Exposure>().is_none());
        assert!(fctx.contains::<FrameLabel>());
    }
}
