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

//! Acts as the **[A]gent** for the asset subsystem, fulfilling the role of an ISA.
//!
//! This module provides the high-level, tactical logic for asset management in Khora.
//! It is the public-facing API for requesting assets and querying their state, but
//! it delegates the actual, heavy-lifting work of loading to the `asset_lane`.
//!
//! As an **Intelligent Subsystem Agent (ISA)**, its future responsibilities will include:
//! - Reporting its own resource usage (e.g., memory, I/O bandwidth) to the DCC.
//! - Managing multiple loading strategies (e.g., synchronous vs. asynchronous,
//!   high-quality vs. low-quality) and selecting one based on the budget allocated by GORNA.
//!
//! The primary entry point for users is the `AssetServer` resource, which orchestrates
//! the asset loading lifecycle.
