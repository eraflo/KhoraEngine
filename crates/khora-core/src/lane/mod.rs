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
//!    - `AssetLoaderLane<A>: Lane` — Asset loading strategies
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

pub mod context_keys;
pub use context_keys::*;

/// Error type for lane operations.
#[derive(Debug)]
pub enum LaneError {
    /// The lane has not been initialized yet.
    NotInitialized,
    /// The execution context passed to the lane has the wrong type.
    InvalidContext {
        /// What the lane expected.
        expected: &'static str,
        /// Description of what was received.
        received: String,
    },
    /// A domain-specific error occurred during execution.
    ExecutionFailed(Box<dyn std::error::Error + Send + Sync>),
    /// A domain-specific error occurred during initialization.
    InitializationFailed(Box<dyn std::error::Error + Send + Sync>),
}

impl fmt::Display for LaneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LaneError::NotInitialized => write!(f, "Lane not initialized"),
            LaneError::InvalidContext { expected, received } => {
                write!(
                    f,
                    "Invalid lane context: expected {expected}, got {received}"
                )
            }
            LaneError::ExecutionFailed(e) => write!(f, "Lane execution failed: {e}"),
            LaneError::InitializationFailed(e) => write!(f, "Lane initialization failed: {e}"),
        }
    }
}

impl std::error::Error for LaneError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LaneError::ExecutionFailed(e) | LaneError::InitializationFailed(e) => Some(e.as_ref()),
            _ => None,
        }
    }
}

impl LaneError {
    /// Convenience constructor for a missing context entry.
    pub fn missing(type_name: &'static str) -> Self {
        LaneError::InvalidContext {
            expected: type_name,
            received: "not found in LaneContext".into(),
        }
    }
}

/// Classification of lane types, used for routing and filtering.
///
/// Agents use this to identify compatible lanes during GORNA negotiation
/// and lane selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LaneKind {
    /// Main scene rendering (forward, deferred, etc.)
    Render,
    /// Shadow map generation
    Shadow,
    /// Physics simulation
    Physics,
    /// Audio mixing and spatialization
    Audio,
    /// Asset loading and processing
    Asset,
    /// Scene serialization/deserialization
    Scene,
    /// ECS maintenance (compaction, garbage collection)
    Ecs,
}

impl std::fmt::Display for LaneKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LaneKind::Render => write!(f, "Render"),
            LaneKind::Shadow => write!(f, "Shadow"),
            LaneKind::Physics => write!(f, "Physics"),
            LaneKind::Audio => write!(f, "Audio"),
            LaneKind::Asset => write!(f, "Asset"),
            LaneKind::Scene => write!(f, "Scene"),
            LaneKind::Ecs => write!(f, "ECS"),
        }
    }
}

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
/// # Mutable references
///
/// For data that is borrowed (not owned), use [`Slot`] (mutable) or
/// [`Ref`] (shared) wrappers:
///
/// ```rust,ignore
/// use khora_core::lane::{LaneContext, Slot};
///
/// let mut value = 10u32;
/// let mut ctx = LaneContext::new();
/// ctx.insert(Slot::new(&mut value));
///
/// let slot = ctx.get::<Slot<u32>>().unwrap();
/// *slot.get() = 20;
/// ```
///
/// # Safety
///
/// `LaneContext` uses `unsafe impl Send + Sync` because it may hold
/// [`Slot`] / [`Ref`] wrappers containing raw pointers. This is safe
/// because the context is stack-scoped: created by the agent, passed to
/// one lane at a time, and dropped before the next frame.
pub struct LaneContext {
    data: HashMap<TypeId, Box<dyn Any>>,
}

// SAFETY: All values inserted via `insert<T: Send + Sync>()` are Send+Sync.
// Slot/Ref wrappers hold raw pointers but are only used within single-threaded
// frame scopes where the pointed-to data is guaranteed to be alive.
unsafe impl Send for LaneContext {}
unsafe impl Sync for LaneContext {}

impl LaneContext {
    /// Creates an empty context.
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
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

impl Default for LaneContext {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for LaneContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LaneContext")
            .field("entries", &self.data.len())
            .finish()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Slot / Ref — safe-ish wrappers for borrowing through LaneContext
// ─────────────────────────────────────────────────────────────────────────────

/// Wraps a **mutable** borrow for storage in [`LaneContext`].
///
/// This erases the lifetime so the value can be stored in the type-map.
/// The caller **must** ensure the `Slot` does not outlive the original
/// reference (guaranteed by the stack-scoped context pattern).
///
/// ```rust,ignore
/// use khora_core::lane::Slot;
///
/// let mut encoder: Box<dyn CommandEncoder> = /* ... */;
/// let slot = Slot::new(encoder.as_mut());
/// // slot.get() -> &mut dyn CommandEncoder
/// ```
pub struct Slot<T: ?Sized>(*mut T);

// SAFETY: Slot is used only within single-threaded frame scopes.
unsafe impl<T: ?Sized> Send for Slot<T> {}
unsafe impl<T: ?Sized> Sync for Slot<T> {}

impl<T: ?Sized> Slot<T> {
    /// Creates a `Slot` from a mutable reference.
    pub fn new(value: &mut T) -> Self {
        Self(value as *mut T)
    }

    /// Creates a `Slot` from a raw pointer.
    ///
    /// # Safety
    ///
    /// The caller must ensure:
    /// - The pointer is valid and properly aligned.
    /// - The pointed-to data outlives every use of this `Slot`.
    /// - No other mutable reference to the data exists while the `Slot` is live.
    pub unsafe fn from_raw(ptr: *mut T) -> Self {
        Self(ptr)
    }

    /// Returns a mutable reference to the wrapped value.
    ///
    /// # Safety contract
    ///
    /// Safe when called within the scope where the original reference is
    /// still alive and no other reference to the same data exists.
    #[allow(clippy::mut_from_ref)]
    pub fn get(&self) -> &mut T {
        // SAFETY: guaranteed by single-lane-at-a-time execution
        unsafe { &mut *self.0 }
    }

    /// Returns a shared reference to the wrapped value.
    pub fn get_ref(&self) -> &T {
        // SAFETY: same as get()
        unsafe { &*self.0 }
    }
}

/// Wraps a **shared** borrow for storage in [`LaneContext`].
///
/// Like [`Slot`] but for immutable references.
pub struct Ref<T: ?Sized>(*const T);

// SAFETY: Ref is used only within single-threaded frame scopes.
unsafe impl<T: ?Sized> Send for Ref<T> {}
unsafe impl<T: ?Sized> Sync for Ref<T> {}

impl<T: ?Sized> Ref<T> {
    /// Creates a `Ref` from a shared reference.
    pub fn new(value: &T) -> Self {
        Self(value as *const T)
    }

    /// Returns a shared reference to the wrapped value.
    pub fn get(&self) -> &T {
        // SAFETY: guaranteed by frame-scoped lifetime
        unsafe { &*self.0 }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// LaneRegistry — generic container for heterogeneous lanes
// ─────────────────────────────────────────────────────────────────────────────

/// A registry that stores [`Lane`] trait objects for agent use.
///
/// Agents use a `LaneRegistry` instead of domain-specific vectors
/// (e.g., `Vec<Box<dyn RenderLane>>`). This enables developers to add
/// custom lanes without modifying agent code.
///
/// ```rust,ignore
/// use khora_core::lane::{LaneRegistry, LaneKind};
///
/// let mut reg = LaneRegistry::new();
/// reg.register(Box::new(MyCustomLane::new()));
///
/// // Find all render lanes
/// let render_lanes = reg.find_by_kind(LaneKind::Render);
/// ```
pub struct LaneRegistry {
    lanes: Vec<Box<dyn Lane>>,
}

impl LaneRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self { lanes: Vec::new() }
    }

    /// Adds a lane to the registry.
    pub fn register(&mut self, lane: Box<dyn Lane>) {
        self.lanes.push(lane);
    }

    /// Finds a lane by its strategy name.
    pub fn get(&self, name: &str) -> Option<&dyn Lane> {
        self.lanes
            .iter()
            .find(|l| l.strategy_name() == name)
            .map(|b| b.as_ref())
    }

    /// Returns all lanes of a given kind.
    pub fn find_by_kind(&self, kind: LaneKind) -> Vec<&dyn Lane> {
        self.lanes
            .iter()
            .filter(|l| l.lane_kind() == kind)
            .map(|b| b.as_ref())
            .collect()
    }

    /// Returns a slice of all registered lanes.
    pub fn all(&self) -> &[Box<dyn Lane>] {
        &self.lanes
    }

    /// Returns the number of registered lanes.
    pub fn len(&self) -> usize {
        self.lanes.len()
    }

    /// Returns `true` if no lanes are registered.
    pub fn is_empty(&self) -> bool {
        self.lanes.is_empty()
    }
}

impl Default for LaneRegistry {
    fn default() -> Self {
        Self::new()
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
