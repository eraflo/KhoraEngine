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

//! What the inspector shows of components a script declared, and what
//! "+ Add Component" offers.

/// A component a script declared, as the inspector shows it: its fields in
/// declaration order, each with its Ergon type, its value and its default.
#[derive(Debug, Clone, PartialEq)]
pub struct DeclaredCard {
    /// The component's name — card title and lookup key.
    pub name: String,
    /// The domain whose pages hold it.
    pub domain: crate::ecs::SemanticDomain,
    /// The `.erg` module declaring it now, relative to the script root —
    /// `None` when no module does.
    pub module: Option<String>,
    /// The part it plays for scripts.
    pub kind: DeclaredCardKind,
    /// Whether the inspector shows it without letting it be edited.
    pub read_only: bool,
    /// Whether no module declares it any more: kept with its data.
    pub orphaned: bool,
    /// Its fields, in declaration order.
    pub fields: Vec<DeclaredFieldView>,
}

/// The part a declared component plays for scripts, as its card names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclaredCardKind {
    /// A `component` a script declares.
    Component,
    /// A behavior's authored fields.
    Behavior,
    /// A behavior's state companion, written by the engine.
    BehaviorState,
    /// A system's fields, held by its one runtime entity.
    System,
}

/// One field of a [`DeclaredCard`].
#[derive(Debug, Clone, PartialEq)]
pub struct DeclaredFieldView {
    /// Its name.
    pub name: String,
    /// Its Ergon type.
    pub ty: crate::script::ErgonType,
    /// What the entity holds.
    pub value: crate::script::ScriptValue,
    /// What a new row starts with.
    pub default: crate::script::ScriptValue,
    /// How the inspector presents it: its range, tooltip and section header
    /// as its declaring module gives them — none when it gives none.
    pub hints: crate::script::FieldHints,
}

/// One entry of "+ Add Component": a component the entity may be given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddableComponent {
    /// The component's name.
    pub name: String,
    /// The domain whose pages hold it, where known — what the menu groups by.
    pub domain: Option<crate::ecs::SemanticDomain>,
    /// What declares it.
    pub origin: ComponentOrigin,
}

/// What declares a component the inspector offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentOrigin {
    /// A Rust type.
    Rust,
    /// A script.
    Script {
        /// The `.erg` module declaring it, relative to the script root.
        module: String,
    },
}
