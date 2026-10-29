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

use std::any::type_name;

fn is_agent<T: khora_core::agent::Agent>() {}
fn is_clone<T: Clone>() {}
fn is_copy<T: Copy>() {}
fn is_debug<T: std::fmt::Debug>() {}
fn is_default<T: Default>() {}
fn is_eq<T: Eq>() {}
fn is_partial_eq<T: PartialEq>() {}

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

// ---------------------------------------------------------------------------
// Public fields (read through a reference) and enum variants (matched
// exhaustively). Nothing is constructed.
// ---------------------------------------------------------------------------

fn rendering_strategy_variants(x: &khora_agents::render_agent::RenderingStrategy) {
    match x {
        khora_agents::render_agent::RenderingStrategy::Unlit => {}
        khora_agents::render_agent::RenderingStrategy::LitForward => {}
        khora_agents::render_agent::RenderingStrategy::StandardPbr => {}
        khora_agents::render_agent::RenderingStrategy::ForwardPlus => {}
        khora_agents::render_agent::RenderingStrategy::Auto => {}
    }
}

#[test]
fn module_render_agent_paths_still_resolve() {
    let _ = type_name::<khora_agents::render_agent::RenderAgent>();
    is_agent::<khora_agents::render_agent::RenderAgent>();
    is_default::<khora_agents::render_agent::RenderAgent>();
    let _ = type_name::<khora_agents::render_agent::RenderingStrategy>();
    let _ = rendering_strategy_variants as fn(&khora_agents::render_agent::RenderingStrategy);
    is_debug::<khora_agents::render_agent::RenderingStrategy>();
    is_clone::<khora_agents::render_agent::RenderingStrategy>();
    is_copy::<khora_agents::render_agent::RenderingStrategy>();
    is_partial_eq::<khora_agents::render_agent::RenderingStrategy>();
    is_eq::<khora_agents::render_agent::RenderingStrategy>();
    is_default::<khora_agents::render_agent::RenderingStrategy>();
}

#[test]
fn module_script_agent_paths_still_resolve() {
    let _ = type_name::<khora_agents::script_agent::ScriptAgent>();
    is_default::<khora_agents::script_agent::ScriptAgent>();
    is_agent::<khora_agents::script_agent::ScriptAgent>();
}

// ---------------------------------------------------------------------------
// Paths other crates of the workspace spell today (`crates/`, `examples/`,
// `xtask/`; brace imports expanded). The trailing comment names
// the users. Items, modules and enum variants are imported;
// associated items are named in the test below.
// ---------------------------------------------------------------------------

#[test]
fn associated_items_used_by_other_crates_still_resolve() {
    let _ = khora_agents::render_agent::RenderAgent::default; // khora-sdk
    let _ = khora_agents::script_agent::ScriptAgent::default; // khora-sdk
}
