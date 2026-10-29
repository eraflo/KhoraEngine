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

/// A moment of a [`World`](crate::ecs::World)'s clock: one tick per run — of
/// a `DataSystem`, of a `Flow`, of a scheduler wave. A `u64` that never wraps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Tick(pub u64);

/// Someone who watches the world's changes: a `DataSystem` or a `Flow`, by
/// its registered name. Both kinds may share a name, so the kind is part of
/// who it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Observer {
    /// A `DataSystem`, by its `DataSystemRegistration::name`.
    DataSystem(&'static str),
    /// A `Flow`, by its `FlowRegistration::name`.
    Flow(&'static str),
}
