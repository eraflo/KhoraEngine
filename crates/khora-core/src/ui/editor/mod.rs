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

//! Abstract editor framework: panel system, dock layout, UI builder, and theme.
//!
//! All types here are backend-agnostic. Concrete implementations (egui, etc.)
//! live in `khora-infra`.

mod addable_component;
mod asset_file_op;
pub mod declared;
mod font_family_hint;
pub mod gizmo;
pub mod gradient;
pub mod icons;
mod inline_edit_event;
mod inspected;
pub mod interaction;
mod key_change;
mod menu_action;
pub mod overlay;
pub mod panel;
mod panel_location;
mod prefab_apply_scope;
mod property_edit;
mod radial_gradient;
mod request;
mod scene_in_project;
mod scene_opening;
pub mod shell;
mod spawn_shape;
pub mod state;
mod text_align;
mod transport;
mod trust_answer;
pub mod ui_builder;
pub mod viewport_texture;

pub use asset_file_op::AssetFileOp;
pub use declared::{
    AddableComponent, ComponentOrigin, DeclaredCard, DeclaredCardKind, DeclaredFieldView,
};
pub use gizmo::GizmoLineInstance;
pub use gradient::{GradientStop, LinearGradient, RadialGradient};
pub use icons::Icon;
pub use key_change::KeyChange;
pub use menu_action::MenuAction;
pub use overlay::{EditorOverlay, OverlayError, OverlayScreenDescriptor};
pub use panel::{EditorPanel, PanelLocation};
pub use request::EditorRequest;
pub use scene_in_project::SceneInProject;
pub use scene_opening::SceneOpening;
pub use shell::EditorShell;
pub use spawn_shape::SpawnShape;
pub use state::{
    AssetEntry, ComponentJson, ComponentOverride, EditorMode, EditorState, EntityIcon, GizmoMode,
    InspectedEntity, InspectedPrefab, LogEntry, LogLevel, PlayMode, PrefabApplyScope, PropertyEdit,
    QuarantinedAsset, SceneNode, StatusBarData, TrustSummary,
};
pub use transport::Transport;
pub use trust_answer::TrustAnswer;
pub use ui_builder::{FontFamilyHint, InlineEditEvent, Interaction, TextAlign, UiBuilder};
pub use viewport_texture::ViewportTextureHandle;
