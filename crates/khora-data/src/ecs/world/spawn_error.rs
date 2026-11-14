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

//! The `SpawnError`: why a bundle could not be spawned.

/// Errors that can occur when spawning an entity with a bundle of components.
#[derive(Debug, PartialEq, Eq)]
pub enum SpawnError {
    /// A component type of the bundle is not registered in the ECS: it has no
    /// domain, so nothing could ever find it once stored.
    ComponentNotRegistered {
        /// The name of the unregistered component type.
        component: &'static str,
    },
    /// The bundle names one component type twice: an entity holds one of each.
    ComponentNamedTwice {
        /// The name of the repeated component type.
        component: &'static str,
    },
}
