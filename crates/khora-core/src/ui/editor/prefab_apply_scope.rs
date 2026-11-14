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

//! What of an instance an apply-to-prefab writes into its prefab.

/// What of an instance an apply-to-prefab writes into its prefab.
#[derive(Debug, Clone, PartialEq)]
pub enum PrefabApplyScope {
    /// One field of one component of the inspected entity.
    Field {
        /// The component's registered type name.
        type_name: String,
        /// Field names from the component's root.
        path: Vec<String>,
    },
    /// One component of the inspected entity, whole.
    Component {
        /// The component's registered type name.
        type_name: String,
    },
    /// Every override of the instance the entity belongs to.
    Instance,
}
