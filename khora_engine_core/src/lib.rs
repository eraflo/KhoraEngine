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

//! Khora Engine Core Crate
//!
//! Provides the foundational runtime of KhoraEngine (main loop, event bus,
//! rendering abstraction, math types, memory tracking) structured with the
//! future Symbiotic Adaptive Architecture (SAA) in mind (see
//! `../docs/architecture_design.md`).
//!
//! Main modules:
//! * [`core`]: runtime engine (loop, event dispatch, timers, basic stats)
//! * [`event`]: event definitions & decoupled publish / subscribe bus
//! * [`math`]: vectors, matrices, quaternions, basic numeric helpers
//! * [`memory`]: heap allocation tracking allocator
//! * [`subsystems`]: pluggable subsystems (renderer `RenderSystem`, input, etc.)
//! * [`window`]: OS window layer via `winit`
//!
//! Immediate extension points:
//! * Add a new subsystem: create a module under `subsystems` and integrate in
//!   `Engine` (event publication, future update hook).
//! * Add instrumentation: extend `RenderStats` (and later the Core Metrics System).
//!
//! Rendering & GPU instrumentation:
//! See `../docs/rendering/gpu_monitoring.md` for comprehensive coverage of
//! backend-agnostic monitoring, WGPU timestamp pipeline, and adaptive resize strategies.
//!
//! Quick design guidelines:
//! * Keep the main loop non-blocking (avoid `std::thread::sleep` inside per-frame path)
//! * Avoid panics outside tests; prefer `Result` or event signaling
//! * Comments should focus on the "why" when logic is non-trivial
//!
//! Public API surface is intentionally small (Engine, EngineEvent, input events) to
//! keep downstream usage minimal and stable during early iterations.

pub mod core;
pub mod event;
pub mod math;
pub mod memory;
pub mod subsystems;
pub mod window;

pub use core::engine::Engine;
pub use event::EngineEvent;
pub use subsystems::input::InputEvent as KhoraInputEvent;
