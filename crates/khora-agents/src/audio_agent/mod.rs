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

//! The Intelligent Subsystem Agent responsible for managing the audio system.
//!
//! Per CLAD the agent owns no hardware state. The audio device is opened
//! by the application during bootstrap; the resulting [`AudioStream`]
//! handle and the shared [`AudioMixBus`] live in the runtime. Each frame
//! the agent reads the per-tick `AudioView` from the `LaneBus`, builds a
//! `LaneContext` containing the bus + the view + a slot to the
//! `OutputDeck`, and dispatches its audio lanes (currently only
//! [`SpatialMixingLane`]). Lanes mix into a staging buffer and push
//! samples to the bus; the backend's audio callback drains the bus on a
//! dedicated real-time thread.
//!
//! [`AudioMixBus`]: khora_core::audio::AudioMixBus
//! [`SpatialMixingLane`]: khora_lanes::audio_lane::SpatialMixingLane

mod agent;

pub use agent::*;
