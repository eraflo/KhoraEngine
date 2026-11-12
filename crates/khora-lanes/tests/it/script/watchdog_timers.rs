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

//! **A timer body the watchdog stops leaves its schedule as a fault does**
//! (Ergon spec 10): a timer whose body faults is rearmed — an `after` has
//! fired and fires no more, an `every` waits its interval — so the edit that
//! starts the instance again does not run that schedule a second time at
//! once.

use super::watchdog_stage::{build, entity, Stage, MODULE, SLICE};

/// Plenty of fuel for a turn that does not spin.
const PLENTY: u64 = 1_000_000;

/// An `after` whose body counts its firing, then spins.
const ONCE: &str = "behavior Once {
                        int fired = 0;
                        int n = 0;
                        after 1s { fired += 1; while (true) { n += 1; } }
                    }";

/// The same `after`, fixed.
const ONCE_FIXED: &str = "behavior Once {
                              int fired = 0;
                              int n = 0;
                              after 1s { fired += 1; }
                          }";

/// **An `after` the watchdog stopped has fired**: once an edit starts the
/// instance again, it does not fire a second time.
#[test]
fn an_after_the_watchdog_stopped_does_not_fire_again_after_the_edit() {
    let subject = entity(0);
    let mut stage = Stage::new(ONCE, &[(subject, "Once")]);
    let stopped = stage.frames_until_disabled(subject, "Once", 1.0, SLICE, 70);
    assert!(stopped.is_some(), "the spinning `after` was stopped");
    assert_eq!(stage.int(subject, "Once", "fired"), Some(1));

    stage.runtime.reload(MODULE, build(ONCE_FIXED));
    stage.frames(10, 1.0, PLENTY);
    assert!(
        !stage.disabled(subject, "Once"),
        "the edit started it again"
    );
    assert_eq!(
        stage.int(subject, "Once", "fired"),
        Some(1),
        "an `after` fires once"
    );
}

/// An `every 5s` whose body counts its firing, then spins.
const EVERY: &str = "behavior Every {
                         int fired = 0;
                         int n = 0;
                         every 5s { fired += 1; while (true) { n += 1; } }
                     }";

/// The same `every`, fixed.
const EVERY_FIXED: &str = "behavior Every {
                               int fired = 0;
                               int n = 0;
                               every 5s { fired += 1; }
                           }";

/// **An `every` the watchdog stopped waits its interval** once an edit
/// starts the instance again: it does not fire on the edit's first frame.
#[test]
fn an_every_the_watchdog_stopped_waits_its_interval_after_the_edit() {
    let subject = entity(0);
    let mut stage = Stage::new(EVERY, &[(subject, "Every")]);
    let stopped = stage.frames_until_disabled(subject, "Every", 1.0, SLICE, 80);
    assert!(stopped.is_some(), "the spinning `every` was stopped");
    let fired = stage.int(subject, "Every", "fired");
    assert_eq!(fired, Some(1));

    stage.runtime.reload(MODULE, build(EVERY_FIXED));
    stage.frames(2, 1.0, PLENTY);
    assert!(
        !stage.disabled(subject, "Every"),
        "the edit started it again"
    );
    assert_eq!(
        stage.int(subject, "Every", "fired"),
        fired,
        "two seconds after the edit, a five-second schedule has not fired"
    );
}
