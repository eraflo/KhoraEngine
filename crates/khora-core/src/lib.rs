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

//! # Khora Core
//!
//! Foundational crate containing traits, core types, and interface contracts
//! that define the engine's architecture.

#![warn(missing_docs)]

pub mod agent;
pub mod asset;
pub mod audio;
pub mod context;
pub mod control;

pub mod ecs;
pub mod event;
pub mod graph;
pub mod math;
pub mod memory;
pub mod physics;
pub mod platform;
pub mod renderer;
pub mod scene;
pub mod telemetry;
pub mod utils;
pub mod vfs;

pub use context::EngineContext;
pub use utils::timer::Stopwatch;
