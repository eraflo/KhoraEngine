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

//! OverlayAgent — generic engine-side agent for overlay / debug
//! rendering passes.
//!
//! Unlike `RenderAgent` (one strategy selected per frame via GORNA),
//! lanes registered here run **in parallel** with the main render pass,
//! after it, with depth-read-only / alpha-blend semantics. Domain
//! examples:
//!
//! - `GizmoLane` — editor / debug line gizmos. Data published on the
//!   `OutputDeck` by the host application (editor, game tooling).
//! - `WireframeLane` — wireframe debug viz (opt-in via `WireframeConfig`).
//!
//! Domain-agnostic by design: the engine never assumes who pushes the
//! data, only that the data slot type is well-known. This keeps the
//! editor crate a pure consumer of engine mechanisms instead of an
//! engine-internal `EditorAgent` (which would invert the engine /
//! application dependency).

mod agent;

pub use agent::*;
