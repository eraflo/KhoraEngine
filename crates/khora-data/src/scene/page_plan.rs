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

//! One page of a load, its names resolved, and the check that a load's pages fit together.
//!
//! See the book: concepts/serialization.md, "Atomic loading".

use std::collections::{HashMap, HashSet};

use khora_core::ecs::PersistentId;

use super::apply::Loaded;
use super::load_catalog::LoadCatalog;
use crate::ecs::{ComponentKey, SemanticDomain};

/// One page of a load, its names resolved.
///
/// Its rows' identities and, per kept column, its position in the file and how it is read.
pub(super) struct PagePlan<'r> {
    pub(super) rows: &'r [PersistentId],
    pub(super) columns: Vec<(usize, Loaded)>,
}

/// Checks that a load's pages fit together; returns why they don't.
///
/// Every entity is listed once, a page holds only listed entities, once each,
/// a component appears once per page, and an entity has one page per domain.
pub(super) fn check_plans(
    world: &LoadCatalog,
    entities: &[PersistentId],
    plans: &[PagePlan<'_>],
) -> Result<(), String> {
    let mut known = HashSet::new();
    for id in entities {
        if !known.insert(*id) {
            return Err(format!("entity {:#x} is recorded twice", id.to_bits()));
        }
    }

    let mut domains_of: HashMap<PersistentId, Vec<SemanticDomain>> = HashMap::new();
    for plan in plans {
        let mut page_domains: Vec<SemanticDomain> = Vec::new();
        for (index, (_, loaded)) in plan.columns.iter().enumerate() {
            // Where the file kept it: what decides whether its pages fit
            // together.
            let domain = match loaded {
                Loaded::Rust(reg) => world.domain_of(ComponentKey::Rust(reg.type_id)),
                Loaded::Declared(load) => Some(load.file_domain),
            };
            let Some(domain) = domain else {
                return Err(format!(
                    "component `{}` is not registered in this world",
                    loaded.name()
                ));
            };
            if plan.columns[..index]
                .iter()
                .any(|(_, known)| known.key() == loaded.key())
            {
                return Err(format!(
                    "component `{}` appears twice in one page",
                    loaded.name()
                ));
            }
            if !page_domains.contains(&domain) {
                page_domains.push(domain);
            }
        }
        let mut in_page = HashSet::new();
        for id in plan.rows {
            if !known.contains(id) {
                return Err(format!(
                    "a page holds entity {:#x}, which the record does not list",
                    id.to_bits()
                ));
            }
            if !in_page.insert(*id) {
                return Err(format!(
                    "entity {:#x} has two rows in one page",
                    id.to_bits()
                ));
            }
            let taken = domains_of.entry(*id).or_default();
            if page_domains.iter().any(|domain| taken.contains(domain)) {
                return Err(format!(
                    "entity {:#x} is in two pages of the same domain",
                    id.to_bits()
                ));
            }
            taken.extend(page_domains.iter().copied());
        }
    }
    Ok(())
}
