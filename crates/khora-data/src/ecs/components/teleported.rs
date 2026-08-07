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

//! Somebody moved this entity, and the simulation has to be told.
//!
//! # Why a declaration and not a comparison
//!
//! The physics sync used to *infer* a teleport: it compared the provider's pose
//! to the entity's world pose and, if they differed by more than a centimetre,
//! assumed somebody had moved it. That cannot work. Integration drift, a
//! parent-frame mismatch, a sub-step overshoot and a genuine teleport all
//! produce "the two values differ", and no threshold separates them — 1 cm was
//! small enough to fire on a fast body's own motion and large enough to
//! silently discard a 3 mm nudge.
//!
//! Moving something is an **act**. The one who performs it knows, and says so.
//!
//! # Why a marker and not a channel
//!
//! The rest of the engine's one-way notifications are
//! [`Channel`](khora_core::event::Channel)s, and this nearly was one. The
//! editor decided it: a gizmo drag runs inside `EngineApp::update`, which
//! receives the `World` and **no `Runtime`** — so it can add a component and
//! cannot reach a channel. Routing the most important producer through a second
//! mechanism to keep the first pure would have been purity bought with a
//! detour.
//!
//! It is `Runtime` provenance, so it never reaches a scene file, and the sync
//! removes it the moment it acts: a flag that outlived its frame would teleport
//! a body back to its authored pose forever.

use khora_macros::Component;
use serde::{Deserialize, Serialize};

/// Marks an entity whose pose an author changed this frame.
///
/// Added by whoever moved it — the editor's gizmo, a script's `SetPosition`,
/// a tool. Consumed and removed by the physics sync, which pushes the authored
/// pose into the provider rather than letting the simulation win.
#[derive(Debug, Clone, Copy, Default, Component, Serialize, Deserialize)]
#[component(domain = Spatial, provenance = Runtime)]
pub struct Teleported;
