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

//! The game an application is.

use serde::{Deserialize, Serialize};

/// The game an application is: its name and its version.
///
/// The application gives it (the engine imposes none): a packed game from its
/// `runtime.json`, the editor from the project's `project.json`, an
/// application that says nothing from its window title and the engine's
/// version. What the engine learns while a game runs is filed under it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameIdentity {
    /// The game's name.
    pub name: String,
    /// The game's version.
    pub version: String,
}
