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

//! The project's prefab catalog, published for the lanes that spawn.

use std::collections::BTreeSet;
use std::sync::Arc;

use khora_core::asset::AssetUUID;
use khora_core::Runtime;

use super::{Flow, Selection};
use crate::ecs::{SemanticDomain, World};
use crate::register_flow;
use crate::scene::PrefabCatalog;

/// The prefabs a frame's spawns may name, or `None` when the runtime holds
/// no catalog.
#[derive(Debug, Clone)]
pub struct PrefabCatalogView {
    /// The catalog's content, shared.
    pub prefabs: Option<Arc<BTreeSet<AssetUUID>>>,
}

/// Publishes [`PrefabCatalogView`].
#[derive(Debug, Default)]
pub struct PrefabCatalogFlow;

impl Flow for PrefabCatalogFlow {
    type View = PrefabCatalogView;
    const DOMAIN: SemanticDomain = SemanticDomain::Script;
    const NAME: &'static str = "prefab_catalog";

    fn project(&self, _world: &World, _sel: &Selection, runtime: &Runtime) -> Self::View {
        PrefabCatalogView {
            prefabs: runtime
                .resources
                .get::<PrefabCatalog>()
                .map(PrefabCatalog::snapshot),
        }
    }
}

register_flow!(PrefabCatalogFlow);
