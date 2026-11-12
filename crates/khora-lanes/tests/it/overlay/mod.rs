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

//! `GridLane`, `WireframeLane` and `GizmoLane`, the three overlay families'
//! lanes, run on the typed contract: fetched from an `InputSource` holding
//! the overlay Views and the frame's target, their pass committed through an
//! `OutputSink`, on a ledger device.

mod failures;
mod fixtures;
mod gizmos;
mod grid;
mod levels;
mod lifecycle;
mod pipelines;
mod wireframe;
