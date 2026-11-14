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

//! The world's clock.

pub use super::column_ticks::ColumnTicks;
pub use super::observer::Observer;

/// The silence, in ticks, after which `ecs_maintenance` frees a column's row
/// ticks ([`World::free_quiet_row_ticks`](crate::ecs::World::free_quiet_row_ticks)):
/// about five seconds of frames at 60 frames a second — a frame of the
/// sandbox scene runs 43 ticks (one per `DataSystem` and `Flow` that runs;
/// agents never write the world, so they open no run). A count of ticks, not
/// of seconds: a column is freed once that much of the world's work went by
/// without writing it.
pub const QUIET_ROW_TICKS: u64 = 12_900;

/// A moment of a [`World`](crate::ecs::World)'s clock: one tick per run — of
/// a `DataSystem`, of a `Flow`. A `u64` that never wraps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Tick(pub u64);
