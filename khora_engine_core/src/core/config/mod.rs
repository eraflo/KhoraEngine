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

//! Engine Configuration Module
//!
//! This module contains all configuration-related functionality for the engine.
//! It provides a centralized place for managing engine settings, user preferences,
//! and system configurations.
//!
//! ## Directory Structure
//!
//! - `metrics/` - JSON configuration files for the metrics system
//!   - `memory_basic.json` - Basic memory monitoring configuration
//!   - `memory_extended.json` - Extended memory monitoring with detailed stats
//!   - `memory_advanced.json` - Advanced monitoring with histograms
//!   - `monitoring_complete.json` - Complete resource monitoring setup
//!
//! ## Note
//!
//! Some preset management functionality is currently implemented in the metrics
//! engine module for convenience. This will be moved to a dedicated configuration
//! manager in the future as the configuration system evolves.

// Future configuration modules will be added here
// pub mod render_config;
// pub mod audio_config;
// pub mod input_config;
