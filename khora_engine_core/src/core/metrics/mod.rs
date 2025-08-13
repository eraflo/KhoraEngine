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

//! Core Metrics System v1 - In-Memory Backend
//!
//! This module provides a thread-safe, high-performance metrics collection
//! system for the KhoraEngine. It supports counters, gauges, and basic
//! histogram functionality with an extensible backend architecture.

pub mod backend;
pub mod config;
pub mod engine;
pub mod memory_backend;
pub mod registry;
pub mod scheduler;
pub mod types;

pub use backend::*;
pub use config::*;
pub use engine::*;
pub use memory_backend::*;
pub use registry::*;
pub use scheduler::*;
pub use types::*;
