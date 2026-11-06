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

//! The script family's agent: its registration, and nothing else.
//!
//! Scripting negotiates for one reason: the DCC has to be able to say "you
//! have 0.4ms, hand back control", and a language that cannot be told that
//! makes the frame budget a suggestion. Ergon can: the lane turns the time it
//! is lent into fuel at the rate it measured, and a behaviour that runs out
//! suspends at a safepoint and resumes next frame.
//!
//! The engine boots one [`LaneAgent`](crate::agent::LaneAgent) from this
//! registration, like every family. Its two levels serve the `[Critical]`
//! behaviours alone, or every due one — the default; both keep the result,
//! since a cut defers a turn and never changes what a turn computes. The lane
//! is world-free: it reads the script Views, locks the scripting world for
//! the run and writes the frame's commands and the instances' state, so the
//! scheduler can run it beside other work.

mod agent;
