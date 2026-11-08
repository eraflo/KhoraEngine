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

//! How a recompiled program reaches the scripting world in these tests: the
//! way every program does, staged by the `loading` family into the runtime's
//! reserve, for the scripting family's intake to take.

use khora_core::lane::{Reserve, Staged, Staging};

/// Stages `item` into `reserve`, as the scheduler hands a compile run's
/// staging over at a frame start.
pub(crate) fn stage<T: Staged>(reserve: &Reserve, item: T) {
    let mut staging = Staging::new();
    staging.stage(vec![item]);
    reserve.receive(staging);
}
