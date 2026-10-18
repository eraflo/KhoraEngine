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

//! The prefabs a running game spawns from, published in the `Runtime`.

use std::sync::Arc;

use super::PrefabSource;

/// Where the frame boundary reads the prefabs a script spawns: a `Runtime`
/// resource. Without one, every prefab spawn is refused.
pub struct PrefabLibrary(pub Arc<dyn PrefabSource + Send + Sync>);
