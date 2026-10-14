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

//! **A system imported by several modules runs once a frame** (Ergon spec
//! 10): every importer's program holds the system's code, and only the
//! module that declares it runs it.

use khora_data::ecs::World;

use super::behavior_session::{int, Project};
use super::system_session::{the_holder, SystemSession};

const SHARED: Project = &[
    (
        "game/clock.erg",
        "system Clock { int ticks = 0; void Update(float dt) { ticks += 1; } }",
    ),
    (
        "game/a.erg",
        "import \"game/clock.erg\";
         behavior A { void Update(float dt) { } }",
    ),
    (
        "game/b.erg",
        "import \"game/clock.erg\";
         behavior B { void Update(float dt) { } }",
    ),
];

/// **Three programs hold `Clock`, one runs it**: after three frames, three
/// ticks.
#[test]
fn a_system_imported_by_two_modules_runs_once_a_frame() {
    let mut session = SystemSession::new(World::new(), SHARED);
    for _ in 0..3 {
        session.frame(0.016);
    }
    let clock = the_holder(&session.world, "Clock");
    assert_eq!(int(&session.world, clock, "Clock", "ticks"), Some(3));
}
