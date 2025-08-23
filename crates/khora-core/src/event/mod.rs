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

//! Provides foundational primitives for event-driven communication.
//!
//! This module contains generic, decoupled components for creating and managing
//! event channels. The primary component is the [`EventBus`], a generic, thread-safe
//! MPSC (multi-producer, single-consumer) channel.
//!
//! By keeping these primitives generic, `khora-core` allows higher-level crates
//! to define their own specific event types without creating circular dependencies.

mod bus;

pub use self::bus::EventBus;
