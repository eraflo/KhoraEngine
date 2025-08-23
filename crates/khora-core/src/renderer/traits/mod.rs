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

//! Defines the core architectural traits for the rendering subsystem.
//!
//! This module contains the fundamental contracts that decouple the engine's rendering
//! logic from any specific graphics backend.
//!
//! - [`GraphicsDevice`]: The main interface for creating and managing GPU resources.
//! - [`CommandRecorder`]: An interface for recording GPU commands.
//! - [`RenderSystem`]: A high-level trait representing the entire rendering pipeline.
//! - [`GraphicsBackendSelector`]: A trait for selecting the appropriate graphics backend.
//! - [`GpuProfiler`]: An interface for performance profiling on the GPU.

mod backend_selector;
mod command_recorder;
mod graphics_device;
mod profiler;
mod render_system;

pub use self::backend_selector::GraphicsBackendSelector;
pub use self::command_recorder::*;
pub use self::graphics_device::GraphicsDevice;
pub use self::profiler::*;
pub use self::render_system::RenderSystem;
