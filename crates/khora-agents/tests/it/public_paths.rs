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

//! Compile-level guard over `khora_agents`'s public surface, plus the exported
//! macros and the paths other crates of the workspace use.
//!
//! Every `pub` item reachable through a `pub` path is named here at its full
//! path: each `pub mod`, the `pub use` re-exports, their types, free functions,
//! constants, type aliases, public fields, enum variants (matched
//! exhaustively), inherent `pub` methods (turbofished where generic), and the
//! trait impls. A reorganisation that moves code between files must keep every
//! one of these paths valid, so this module stops compiling the moment one
//! disappears.
//!
//! Re-exports are also checked for *identity*: a `pub use` that starts pointing
//! at a different item with the same name is a compile error (`same_type` for
//! types, `same_item` for functions).
//!
//! The list was generated from `khora_agents`'s rustdoc JSON, so it is complete for the
//! tree it was written against. Nothing is constructed; the tests only have to
//! type-check.

use std::marker::PhantomData;

// ---------------------------------------------------------------------------
// Every `pub mod`, down to the leaves.
// ---------------------------------------------------------------------------

#[allow(unused_imports)]
mod every_pub_mod {
    use khora_agents::overlay_agent as _;
    use khora_agents::render_agent as _;
    use khora_agents::script_agent as _;
    use khora_agents::skybox_agent as _;
    use khora_agents::ui_agent as _;
}

/// The render agent is the render family's `LaneAgent`: its module keeps
/// only the name the skybox and the overlays wait on.
#[test]
fn module_render_agent_paths_still_resolve() {
    let _: khora_core::agent::gorna::AgentId = khora_agents::render_agent::RENDERER;
}

// ---------------------------------------------------------------------------
// Paths other crates of the workspace spell today (`crates/`, `examples/`,
// `xtask/`; brace imports expanded). The trailing comment names
// the users. Items, modules and enum variants are imported;
// associated items are named in the test below.
// ---------------------------------------------------------------------------

#[test]
fn associated_items_used_by_other_crates_still_resolve() {
    let _ = khora_agents::render_agent::RENDERER; // khora-sdk
}

// ---------------------------------------------------------------------------
// The registration collection, re-exported from `khora_core`.
// ---------------------------------------------------------------------------

/// Compiles only when both arguments name the same type.
fn same_type<T: ?Sized>(_: PhantomData<T>, _: PhantomData<T>) {}

/// Compiles only when both references point at the same item (two distinct
/// `fn` items never share a type, and `&fn-item` does not coerce).
fn same_item<T>(_: &T, _: &T) {}

/// `khora_agents::registered_agent` re-exports the collection that lives in
/// `khora_core::agent::registration`: one newtype, one reading function.
#[test]
fn registered_agent_is_the_core_registered_agent() {
    same_type(
        PhantomData::<khora_agents::registered_agent::RegisteredAgent>,
        PhantomData::<khora_core::agent::registration::RegisteredAgent>,
    );
    same_item(
        &khora_agents::registered_agent::registered_agents,
        &khora_core::agent::registration::registered_agents,
    );
    let _ = khora_agents::registered_agent::lane_agents;
    let _ = khora_agents::registered_agent::lane_agents_in;
}

/// The families submitted in this binary are one collection: reading it
/// through `khora_core` or through `khora_agents` gives the same
/// registrations, in the same order, at the same addresses.
#[test]
fn core_and_agents_read_the_same_registrations() {
    let from_core = khora_core::agent::registration::registered_agents();
    let from_agents = khora_agents::registered_agent::registered_agents();
    assert!(
        !from_core.is_empty(),
        "no family registration reached the core collection"
    );
    assert_eq!(from_core.len(), from_agents.len());
    assert!(from_core
        .iter()
        .zip(&from_agents)
        .all(|(core, agents)| std::ptr::eq(*core, *agents)));
}
