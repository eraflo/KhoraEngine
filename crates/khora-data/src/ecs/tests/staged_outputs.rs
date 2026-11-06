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

//! `SystemContext::staged`: what a `Boundary` system takes of what the
//! background runs staged.

use crate::ecs::{SystemContext, Tick};
use khora_core::event::Supersedes;
use khora_core::lane::{Reserve, Staged, Staging};
use khora_core::Runtime;

/// A declaration a background compile staged.
#[derive(Debug, Clone, PartialEq)]
struct Declared(&'static str);
impl Supersedes for Declared {}
impl Staged for Declared {}

/// A context as the runner hands it to a system on its first run.
fn context() -> SystemContext {
    SystemContext {
        since: Tick::default(),
        resumed: true,
        scope: None,
    }
}

/// **A boundary system takes what the reserve holds of its type**, oldest
/// first, and takes it once: the reserve is left empty of it.
#[test]
fn staged_takes_what_the_reserve_holds_of_its_type() {
    let reserve = Reserve::new();
    let mut staging = Staging::new();
    staging.stage(vec![Declared("Guard"), Declared("Door")]);
    reserve.receive(staging);
    let mut runtime = Runtime::new();
    runtime.resources.insert(reserve.clone());

    assert_eq!(
        context().staged::<Declared>(&runtime),
        vec![Declared("Guard"), Declared("Door")]
    );
    assert!(reserve.is_empty::<Declared>(), "taken, not read");
    assert!(context().staged::<Declared>(&runtime).is_empty());
}

/// **A runtime with no reserve stages nothing**: a host without background
/// work runs its boundary systems on nothing.
#[test]
fn staged_on_a_runtime_without_a_reserve_is_empty() {
    let runtime = Runtime::new();

    assert!(context().staged::<Declared>(&runtime).is_empty());
}
