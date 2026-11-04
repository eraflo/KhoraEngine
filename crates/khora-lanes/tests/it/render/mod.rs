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

//! The render family's three lanes — `SimpleUnlitLane`, `LitForwardLane`,
//! `ForwardPlusLane` — run on the typed contract: fetched from an
//! `InputSource` holding the frame's target and clear colour and the shadow
//! frame on the deck, their two passes committed through an `OutputSink`
//! whose passes open on a ledger device and lend its pipeline system.

mod bindings;
mod cost;
mod failures;
mod fixtures;
mod forward_plus;
mod forward_plus_group_3;
mod levels;
mod lifecycle;
mod light_buffer_capacity;
mod passes;
mod pipelines;
