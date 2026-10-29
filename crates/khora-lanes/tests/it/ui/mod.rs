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

//! `UiLane`, the UI family's lane, runs on the typed contract: fetched from
//! an `InputSource` holding the `UiScene` and the frame's target, the image
//! atlas and the text renderer, its pass committed through an `OutputSink`,
//! on a ledger device: the projection for the target's size, the quads as
//! instanced strips, the text flushed into the pass, the atlas bind group,
//! the pipeline per target, the cost, and what the lane builds and gives
//! back.

mod atlas;
mod cost;
mod failures;
mod fallback;
mod fixtures;
mod levels;
mod lifecycle;
mod passes;
mod pipelines;
mod projection;
mod text;
