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

//! Foundational, abstract components for Khora's audio system.
//!
//! - [`device`] — `AudioDevice` (factory) + `AudioStream` (handle).
//! - [`mix_bus`] — `AudioMixBus` cross-thread bridge between audio lanes
//!   (main thread) and the backend's hardware callback (RT thread), plus
//!   a default mutex-backed implementation.

pub mod device;
pub mod mix_bus;

pub use device::{AudioDevice, AudioStream, StreamInfo};
pub use mix_bus::{AudioMixBus, DefaultMixBus};
