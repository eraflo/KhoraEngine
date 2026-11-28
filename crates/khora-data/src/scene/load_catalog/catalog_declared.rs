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

//! A component declared while the engine runs, as a load catalogue knows it.

use std::sync::Arc;

use crate::ecs::{ComponentKey, PackedLayout, SemanticDomain};

/// A component declared while the engine runs, as the catalogue knows it.
#[derive(Debug, Clone)]
pub struct CatalogDeclared {
    /// Its key in the world's pages.
    pub key: ComponentKey,
    /// The domain the world keeps it in.
    pub domain: SemanticDomain,
    /// Its fields, as the world lays its rows out.
    pub layout: Arc<PackedLayout>,
    /// Whether a game save keeps it to resume.
    pub resumable: bool,
}
