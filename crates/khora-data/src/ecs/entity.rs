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

use std::collections::HashMap;

use crate::ecs::{page::PageIndex, SemanticDomain};

/// Represents the central record for an entity, acting as a "table of contents"
/// that points to the physical location of its component data across various `ComponentPage`s.
#[derive(Debug, Clone, Default)]
pub struct EntityMetadata {
    /// A map from a semantic domain to the location of the component data for that domain.
    /// This allows an entity to have component data in multiple different pages,
    /// each specialized for a specific domain (e.g., Spatial, Render).
    pub(crate) locations: HashMap<SemanticDomain, PageIndex>,
}
