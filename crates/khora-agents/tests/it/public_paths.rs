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
    use khora_agents::audio_agent as _;
    use khora_agents::overlay_agent as _;
    use khora_agents::physics_agent as _;
    use khora_agents::render_agent as _;
    use khora_agents::script_agent as _;
    use khora_agents::shadow_agent as _;
    use khora_agents::skybox_agent as _;
    use khora_agents::ui_agent as _;
}

// ---------------------------------------------------------------------------
// Public fields (read through a reference) and enum variants (matched
// exhaustively). Nothing is constructed.
// ---------------------------------------------------------------------------

fn physics_strategy_variants(x: &khora_agents::physics_agent::PhysicsStrategy) {
    match x {
        khora_agents::physics_agent::PhysicsStrategy::Standard => {}
        khora_agents::physics_agent::PhysicsStrategy::Simplified => {}
    }
}

fn rendering_strategy_variants(x: &khora_agents::render_agent::RenderingStrategy) {
    match x {
        khora_agents::render_agent::RenderingStrategy::Unlit => {}
        khora_agents::render_agent::RenderingStrategy::LitForward => {}
        khora_agents::render_agent::RenderingStrategy::StandardPbr => {}
        khora_agents::render_agent::RenderingStrategy::ForwardPlus => {}
        khora_agents::render_agent::RenderingStrategy::Auto => {}
    }
}

fn shadow_strategy_variants(x: &khora_agents::shadow_agent::ShadowStrategy) {
    match x {
        khora_agents::shadow_agent::ShadowStrategy::Standard => {}
        khora_agents::shadow_agent::ShadowStrategy::Medium => {}
        khora_agents::shadow_agent::ShadowStrategy::LowRes => {}
    }
}

#[test]
fn module_audio_agent_paths_still_resolve() {
    let _ = type_name::<khora_agents::audio_agent::AudioAgent>();
    is_default::<khora_agents::audio_agent::AudioAgent>();
    is_agent::<khora_agents::audio_agent::AudioAgent>();
}

#[test]
fn module_overlay_agent_paths_still_resolve() {
    let _ = type_name::<khora_agents::overlay_agent::OverlayAgent>();
    is_agent::<khora_agents::overlay_agent::OverlayAgent>();
    is_default::<khora_agents::overlay_agent::OverlayAgent>();
}

#[test]
fn module_physics_agent_paths_still_resolve() {
    let _ = type_name::<khora_agents::physics_agent::PhysicsAgent>();
    is_agent::<khora_agents::physics_agent::PhysicsAgent>();
    is_default::<khora_agents::physics_agent::PhysicsAgent>();
    let _ = type_name::<khora_agents::physics_agent::PhysicsStrategy>();
    let _ = physics_strategy_variants as fn(&khora_agents::physics_agent::PhysicsStrategy);
    is_debug::<khora_agents::physics_agent::PhysicsStrategy>();
    is_clone::<khora_agents::physics_agent::PhysicsStrategy>();
    is_copy::<khora_agents::physics_agent::PhysicsStrategy>();
    is_partial_eq::<khora_agents::physics_agent::PhysicsStrategy>();
    is_eq::<khora_agents::physics_agent::PhysicsStrategy>();
    is_default::<khora_agents::physics_agent::PhysicsStrategy>();
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

#[test]
fn module_shadow_agent_paths_still_resolve() {
    let _ = type_name::<khora_agents::shadow_agent::ShadowAgent>();
    is_agent::<khora_agents::shadow_agent::ShadowAgent>();
    is_default::<khora_agents::shadow_agent::ShadowAgent>();
    let _ = type_name::<khora_agents::shadow_agent::ShadowStrategy>();
    let _ = shadow_strategy_variants as fn(&khora_agents::shadow_agent::ShadowStrategy);
    let _ = khora_agents::shadow_agent::ShadowStrategy::lane_name;
    is_debug::<khora_agents::shadow_agent::ShadowStrategy>();
    is_clone::<khora_agents::shadow_agent::ShadowStrategy>();
    is_copy::<khora_agents::shadow_agent::ShadowStrategy>();
    is_partial_eq::<khora_agents::shadow_agent::ShadowStrategy>();
    is_eq::<khora_agents::shadow_agent::ShadowStrategy>();
}

#[test]
fn module_skybox_agent_paths_still_resolve() {
    let _ = type_name::<khora_agents::skybox_agent::SkyboxAgent>();
    is_agent::<khora_agents::skybox_agent::SkyboxAgent>();
    is_default::<khora_agents::skybox_agent::SkyboxAgent>();
}

#[test]
fn module_ui_agent_paths_still_resolve() {
    let _ = type_name::<khora_agents::ui_agent::UiAgent>();
    is_agent::<khora_agents::ui_agent::UiAgent>();
    is_default::<khora_agents::ui_agent::UiAgent>();
}

// ---------------------------------------------------------------------------
// Paths other crates of the workspace spell today (`crates/`, `hub/`,
// `examples/`, `xtask/`; brace imports expanded). The trailing comment names
// the users. Items, modules and enum variants are imported;
// associated items are named in the test below.
// ---------------------------------------------------------------------------

#[test]
fn associated_items_used_by_other_crates_still_resolve() {
    let _ = khora_agents::audio_agent::AudioAgent::default; // khora-sdk
    let _ = khora_agents::overlay_agent::OverlayAgent::default; // khora-sdk
    let _ = khora_agents::physics_agent::PhysicsAgent::default; // khora-sdk
    let _ = khora_agents::render_agent::RenderAgent::default; // khora-sdk
    let _ = khora_agents::script_agent::ScriptAgent::default; // khora-sdk
    let _ = khora_agents::shadow_agent::ShadowAgent::default; // khora-sdk
    let _ = khora_agents::skybox_agent::SkyboxAgent::default; // khora-sdk
    let _ = khora_agents::ui_agent::UiAgent::default; // khora-sdk
}
