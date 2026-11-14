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

//! The Substrate Pass: the runners of the data layer's `DataSystem`s and projection `Flow`s.
//! See the book: concepts/the-frame.md, "The Substrate Pass".

pub mod flow_runner;

pub use flow_runner::{flow_last_seen, run_flows};

mod data_system_runner;

pub use data_system_runner::{data_system_last_seen, run_data_systems};

#[cfg(test)]
mod tests;
