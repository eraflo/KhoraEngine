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

//! `core` module – immediate runtime nucleus.
//!
//! Contents:
//! * [`engine`]: main orchestrator (loop, event dispatch, render invocation)
//! * [`monitoring`]: stats / hooks (will evolve into full metrics backend)
//! * [`metrics`]: centralized metrics system with thread-safe backends
//! * [`timer`]: lightweight CPU timing primitives (`Stopwatch`)
//! * [`utils`]: generic helpers (subject to future refactor as scope grows)
//!
//! Note: Future adaptive SAA components (DCC, negotiation, etc.) are not
//! implemented yet; this module provides insertion points for metrics
//! collection & decisions (e.g. after `update`, before `render`).

pub mod config;
pub mod engine;
pub mod metrics;
pub mod monitoring;
pub mod resource_monitors;
pub mod timer;
pub mod utils;
