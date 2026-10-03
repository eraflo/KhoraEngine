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

//! Component types a project no longer has, on purpose.
//!
//! A save holding a component whose type is gone is an error — unless the
//! type was retired deliberately and said so here, in which case the load
//! skips it and reports it. A removal is a decision, and the decision is
//! written down.

/// A component type removed on purpose, by the name saves hold it under.
#[derive(Debug, Clone, Copy)]
pub struct RetiredComponent {
    /// The type name saves wrote.
    pub name: &'static str,
}

inventory::collect!(RetiredComponent);

/// Whether saves may hold `name` without it being an error.
pub fn is_retired(name: &str) -> bool {
    inventory::iter::<RetiredComponent>
        .into_iter()
        .any(|retired| retired.name == name)
}
