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

//! # Lane Abstraction
//!
//! The unified base trait for all lane types in the KhoraEngine.
//!
//! A **Lane** is a reusable, swappable processing strategy within an agent.
//! Agents compose and select lanes based on resource budgets (GORNA protocol)
//! and quality targets. Each lane encapsulates a specific algorithmic approach
//! to a domain task (rendering, physics, audio, asset loading, etc.).
//!
//! ## Architecture
//!
//! The Lane system follows a two-level trait hierarchy:
//!
//! 1. **`Lane`** (this trait) — Common interface shared by ALL lane types.
//!    Provides identity, classification, and cost estimation.
//!
//! 2. **Domain-specific traits** — Extend `Lane` with domain-specific execution
//!    methods. Examples:
//!    - `RenderLane: Lane` — GPU rendering strategies
//!    - `ShadowLane: Lane` — Shadow map generation strategies
//!    - `PhysicsLane: Lane` — Physics simulation strategies
//!    - `AudioMixingLane: Lane` — Audio mixing strategies
//!    - `AssetDecoder<A>` — Asset decoding (bytes → typed asset)
//!    - `SerializationStrategy: Lane` — Scene serialization strategies
//!
//! ## Usage
//!
//! ```rust,ignore
//! use khora_core::lane::{Lane, LaneKind, LaneError, LaneContext};
//!
//! struct MyCustomLane { initialized: std::sync::atomic::AtomicBool }
//!
//! impl Lane for MyCustomLane {
//!     fn strategy_name(&self) -> &'static str { "MyCustom" }
//!     fn lane_kind(&self) -> LaneKind { LaneKind::Render }
//!
//!     fn on_initialize(&self, _ctx: &mut LaneContext) -> Result<(), LaneError> {
//!         self.initialized.store(true, std::sync::atomic::Ordering::Relaxed);
//!         Ok(())
//!     }
//!
//!     fn execute(&self, _ctx: &mut LaneContext) -> Result<(), LaneError> {
//!         // Domain-specific work here
//!         Ok(())
//!     }
//!
//!     fn as_any(&self) -> &dyn std::any::Any { self }
//!     fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
//! }
//! ```

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;
use std::marker::PhantomData;

mod error;
mod kind;
mod registry;

pub use error::LaneError;
pub use kind::LaneKind;
pub use registry::LaneRegistry;

pub mod bus;

pub mod context_keys;

pub mod deck;

pub mod lock;

pub mod slot;

pub use bus::LaneBus;

pub use context_keys::*;

pub use deck::OutputDeck;

pub use lock::{
    mutex_lock, mutex_lock_render, read_lock, read_lock_render, write_lock, write_lock_render,
};

pub use slot::SlotGuard;

// ─────────────────────────────────────────────────────────────────────────────
// LaneContext — generic type-map for passing data to lanes
// ─────────────────────────────────────────────────────────────────────────────

/// A type-erased, extensible context for passing data to lanes.
///
/// Agents populate a `LaneContext` with the data their lanes need,
/// then pass it to [`Lane::execute`], [`Lane::on_initialize`], etc.
/// Lanes retrieve specific data by type using [`get`](LaneContext::get).
///
/// # Adding data
///
/// ```rust,ignore
/// use khora_core::lane::LaneContext;
///
/// let mut ctx = LaneContext::new();
/// ctx.insert(42u32);
/// ctx.insert(String::from("hello"));
///
/// assert_eq!(ctx.get::<u32>(), Some(&42));
/// assert_eq!(ctx.get::<String>().unwrap(), "hello");
/// ```
///
/// # Borrowed data
///
/// Data the agent only borrows is lent for the context's lifetime `'a`:
/// [`insert_slot`](LaneContext::insert_slot) for a mutable borrow, taken back
/// by the lane as a [`SlotGuard`] through [`slot`](LaneContext::slot), and
/// [`insert_ref`](LaneContext::insert_ref) for a shared one, read with
/// [`get_ref`](LaneContext::get_ref). The borrow checker keeps the context from
/// outliving what it borrows.
///
/// ```rust
/// use khora_core::lane::LaneContext;
///
/// let mut value = 10u32;
/// {
///     let mut ctx = LaneContext::new();
///     ctx.insert_slot(&mut value);
///     *ctx.slot::<u32>().unwrap() = 20;
/// }
/// assert_eq!(value, 20);
/// ```
///
/// A context is a local of one `execute`: it never crosses a thread, so it is
/// neither `Send` nor `Sync`.
///
/// ```rust,compile_fail,E0277
/// use khora_core::lane::LaneContext;
///
/// fn assert_send<T: Send>() {}
/// assert_send::<LaneContext<'static>>();
/// ```
///
/// ```rust,compile_fail,E0277
/// use khora_core::lane::LaneContext;
///
/// fn assert_sync<T: Sync>() {}
/// assert_sync::<LaneContext<'static>>();
/// ```
pub struct LaneContext<'a> {
    data: HashMap<TypeId, Box<dyn Any>>,
    _borrows: PhantomData<&'a mut ()>,
}

impl<'a> LaneContext<'a> {
    /// Creates an empty context.
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
            _borrows: PhantomData,
        }
    }

    /// Inserts a value, keyed by its concrete type.
    ///
    /// If a value of the same type was already present, it is replaced.
    pub fn insert<T: 'static + Send + Sync>(&mut self, value: T) {
        self.data.insert(TypeId::of::<T>(), Box::new(value));
    }

    /// Returns a shared reference to a value by type.
    pub fn get<T: 'static>(&self) -> Option<&T> {
        self.data.get(&TypeId::of::<T>())?.downcast_ref()
    }

    /// Returns a mutable reference to a value by type.
    pub fn get_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.data.get_mut(&TypeId::of::<T>())?.downcast_mut()
    }

    /// Checks whether a value of the given type is present.
    pub fn contains<T: 'static>(&self) -> bool {
        self.data.contains_key(&TypeId::of::<T>())
    }

    /// Removes and returns a value by type.
    pub fn remove<T: 'static>(&mut self) -> Option<T> {
        self.data
            .remove(&TypeId::of::<T>())
            .and_then(|b| b.downcast().ok().map(|b| *b))
    }
}

impl Default for LaneContext<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for LaneContext<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LaneContext")
            .field("entries", &self.data.len())
            .finish()
    }
}

/// Base trait for ALL lane types in the KhoraEngine.
///
/// Every lane — regardless of domain — implements this trait, providing
/// a common interface for identity, classification, lifecycle, and execution.
/// This enables agents to reason about lanes generically during GORNA
/// resource negotiation.
///
/// ## Lifecycle
///
/// ```text
/// on_initialize(ctx)  →  [ execute(ctx) ]*  →  on_shutdown(ctx)
/// ```
///
/// - **`on_initialize`** is called once when the lane is registered with an agent
///   or when the underlying device/context changes.
/// - **`execute`** is the main entry point, called each frame/tick by the owning agent.
/// - **`on_shutdown`** is called when the lane is unregistered or the agent shuts down.
///
/// ## LaneContext
///
/// All lifecycle methods receive a [`LaneContext`] — a type-map where
/// agents insert domain-specific data and lanes retrieve it by type.
/// This decouples agents from domain-specific lane traits.
pub trait Lane: Send + Sync {
    /// Human-readable name identifying this lane's strategy.
    ///
    /// Used for logging, debugging, and GORNA negotiation.
    /// Should be unique within a lane kind (e.g., `"LitForward"`, `"StandardPhysics"`).
    fn strategy_name(&self) -> &'static str;

    /// The kind of processing this lane performs.
    ///
    /// Used by agents to classify and route lanes to the appropriate
    /// execution context.
    fn lane_kind(&self) -> LaneKind;

    /// Estimated computational cost of running this lane.
    ///
    /// Used by agents during GORNA resource negotiation to select
    /// lanes that fit within their allocated budget. Higher values
    /// indicate more expensive strategies.
    ///
    /// Default returns `1.0` (medium cost). Override for more
    /// accurate estimation. The [`LaneContext`] may contain scene data
    /// needed for a more precise estimate.
    fn estimate_cost(&self, _ctx: &LaneContext) -> f32 {
        1.0
    }

    // --- Lifecycle ---

    /// Called once when the lane is registered or the underlying context resets.
    ///
    /// The [`LaneContext`] contains domain-specific resources. For example,
    /// render lanes expect an `Arc<dyn GraphicsDevice>` in the context.
    ///
    /// Default is a no-op returning `Ok(())`.
    fn on_initialize(&self, _ctx: &mut LaneContext) -> Result<(), LaneError> {
        Ok(())
    }

    /// Main execution entry point — called each frame/tick by the owning agent.
    ///
    /// The [`LaneContext`] carries all the data the lane needs to do its work.
    /// Lanes extract typed values using `ctx.get::<T>()`.
    ///
    /// Default is a no-op returning `Ok(())`.
    fn execute(&self, _ctx: &mut LaneContext) -> Result<(), LaneError> {
        Ok(())
    }

    /// Called when the lane is being destroyed or the context is shutting down.
    ///
    /// Default is a no-op.
    fn on_shutdown(&self, _ctx: &mut LaneContext) {}

    // --- Downcasting ---

    /// Downcast to a concrete type for type-specific operations.
    fn as_any(&self) -> &dyn Any;

    /// Downcast to a concrete type (mutable) for type-specific operations.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
