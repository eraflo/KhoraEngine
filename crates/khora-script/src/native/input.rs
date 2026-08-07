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

//! Asking what the player is doing.
//!
//! A script names an **action**, never a key: `Pressed("jump")`, not
//! `Pressed(Space)`. The binding is the player's to change and the engine's to
//! keep, and a script that named a key would be a script that has to be edited
//! when somebody rebinds one.
//!
//! # How it gets here
//!
//! The lane is `AgentAccess::Isolated` and reaches no `Runtime`, so a native
//! cannot ask the `InputMap` anything — which is exactly what makes the lane
//! schedulable in parallel. `ScriptFlow` projects a snapshot into the view
//! instead, the same road a transform takes, and the lane puts it on the host
//! once per frame.
//!
//! Reading a snapshot rather than the map is also what makes it *true*: every
//! behavior in a frame sees the same input, and one that read halfway through
//! would be reading something no player did.

use khora_macros::ergon_fn;

use super::{NativeContext, NativeError};

/// `Pressed(string action) -> bool` — whether the action is held right now.
///
/// The question movement asks, every frame it runs.
#[ergon_fn]
fn pressed(context: &mut NativeContext<'_>, action: String) -> Result<bool, NativeError> {
    Ok(context.input.is_pressed(&action))
}

/// `JustPressed(string action) -> bool` — whether it became held this frame.
///
/// The question a jump asks. Distinct from [`pressed`] because holding a key is
/// not pressing it repeatedly, and a behavior that could not tell them apart
/// would jump every frame the key was down.
#[ergon_fn]
fn just_pressed(context: &mut NativeContext<'_>, action: String) -> Result<bool, NativeError> {
    Ok(context.input.just_pressed(&action))
}
