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

//! The address of an entity's row in a component page.

/// A logical address pointing to an entity's component data within a specific `ComponentPage`.
///
/// This struct is the core of the relational aspect of our ECS. It decouples an entity's
/// identity from the physical storage of its data by acting as a coordinate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PageIndex {
    /// The unique identifier of the `ComponentPage` that stores the component data.
    pub page_id: u32,
    /// The index of the row within the page where this entity's components are stored.
    pub row_index: u32,
}
