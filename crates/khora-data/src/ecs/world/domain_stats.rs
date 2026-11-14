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

//! Statistics for a semantic domain.

/// Simple statistics for a semantic domain.
#[derive(Debug, Default, Clone, Copy)]
pub struct DomainStats {
    /// Total number of entities in this domain.
    pub entity_count: u32,
    /// Total number of pages allocated for this domain.
    pub page_count: u32,
}
