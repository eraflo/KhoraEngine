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

//! # UI Layout System
//!
//! Computes layouts for all entities with a `UiNode` and writes the results to `UiTransform`.

use khora_core::ecs::entity::EntityId;
use khora_core::math::Vec2;
use khora_core::ui::layout::UiLayoutView;
use khora_core::ui::types::{UiFlexDirection, UiNode, UiTransform, UiVal};
use khora_core::ui::LayoutSystem;
use std::any::Any;
use std::collections::HashMap;
use taffy::prelude::*;

/// Layout system implementation using the Taffy layout engine.
pub struct TaffyLayoutSystem {
    taffy: TaffyTree,
    entity_to_node: HashMap<EntityId, NodeId>,
}

impl Default for TaffyLayoutSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl TaffyLayoutSystem {
    /// Creates a new TaffyLayoutSystem.
    pub fn new() -> Self {
        Self {
            taffy: TaffyTree::new(),
            entity_to_node: HashMap::new(),
        }
    }
}

impl LayoutSystem for TaffyLayoutSystem {
    fn compute_layouts(&mut self, view: &mut dyn UiLayoutView) {
        self.taffy.clear();
        self.entity_to_node.clear();

        // 1. First pass: Create taffy nodes for all UI entities
        let entities = view.get_all_ui_entities();
        for entity in entities.iter() {
            if let Some(ui_node) = view.get_node(*entity) {
                let style = self.convert_style(&ui_node);
                let node = self
                    .taffy
                    .new_leaf(style)
                    .expect("Failed to create layout node");
                self.entity_to_node.insert(*entity, node);
            }
        }

        // 2. Second pass: Build hierarchy
        let mut roots = Vec::new();

        for &entity in entities.iter() {
            if view.has_parent(entity) {
                continue;
            }
            roots.push(entity);
            self.attach_children(entity, view);
        }

        // 3. Compute layout
        for root in roots {
            if let Some(&root_node) = self.entity_to_node.get(&root) {
                // Determine root size (could be screen size in real app)
                let available_space = Size {
                    width: AvailableSpace::Definite(1920.0), // TODO: Use actual viewport size
                    height: AvailableSpace::Definite(1080.0),
                };

                let _ = self.taffy.compute_layout(root_node, available_space);
                self.update_transforms(root, view, 0); // 0 Z-Index base
            }
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// SAFETY: TaffyTree contains *const () which makes it !Send/!Sync.
// However, we only access it from the ISA thread in a deterministic way.
// In Taffy 0.5+ it should be Send+Sync if we use the default generic.
// If it still complains, we might need these:
unsafe impl Send for TaffyLayoutSystem {}
unsafe impl Sync for TaffyLayoutSystem {}

impl TaffyLayoutSystem {
    fn attach_children(&mut self, entity: EntityId, view: &dyn UiLayoutView) {
        let children = view.get_children(entity);
        if !children.is_empty() {
            let mut taffy_children = Vec::new();
            for &child_id in &children {
                if self.entity_to_node.contains_key(&child_id) {
                    self.attach_children(child_id, view);
                    taffy_children.push(*self.entity_to_node.get(&child_id).unwrap());
                }
            }

            if !taffy_children.is_empty() {
                let parent_node = *self.entity_to_node.get(&entity).unwrap();
                self.taffy.set_children(parent_node, &taffy_children).ok();
            }
        }
    }

    fn update_transforms(&self, entity: EntityId, view: &mut dyn UiLayoutView, z_index: i32) {
        if let Some(&node_id) = self.entity_to_node.get(&entity) {
            if let Ok(layout) = self.taffy.layout(node_id) {
                let transform = UiTransform {
                    pos: Vec2::new(layout.location.x, layout.location.y),
                    size: Vec2::new(layout.size.width, layout.size.height),
                    z_index,
                };

                // Add parent's absolute position logic?
                // Taffy layout is relative to parent by default in some configurations.
                // In our implementation we assume absolute screen coordinates are needed.
                // However, Taffy gives local coords.

                // Writing transform back
                view.set_transform(entity, transform);
            }
        }

        let children = view.get_children(entity);
        for child_id in children {
            self.update_transforms(child_id, view, z_index + 1);
        }
    }

    fn convert_style(&self, node: &UiNode) -> Style {
        Style {
            size: Size {
                width: self.convert_val(node.width),
                height: self.convert_val(node.height),
            },
            min_size: Size {
                width: self.convert_val(node.min_width),
                height: self.convert_val(node.min_height),
            },
            max_size: Size {
                width: self.convert_val(node.max_width),
                height: self.convert_val(node.max_height),
            },
            padding: Rect {
                left: self.convert_length_percentage(node.padding.left),
                right: self.convert_length_percentage(node.padding.right),
                top: self.convert_length_percentage(node.padding.top),
                bottom: self.convert_length_percentage(node.padding.bottom),
            },
            margin: Rect {
                left: self.convert_length_percentage_auto(node.margin.left),
                right: self.convert_length_percentage_auto(node.margin.right),
                top: self.convert_length_percentage_auto(node.margin.top),
                bottom: self.convert_length_percentage_auto(node.margin.bottom),
            },
            flex_direction: match node.flex_direction {
                UiFlexDirection::Column => FlexDirection::Column,
                UiFlexDirection::Row => FlexDirection::Row,
            },
            flex_grow: node.flex_grow,
            flex_shrink: node.flex_shrink,
            ..Default::default()
        }
    }

    fn convert_val(&self, val: UiVal) -> Dimension {
        match val {
            UiVal::Px(v) => length(v),
            UiVal::Percent(v) => percent(v / 100.0),
            UiVal::Auto => auto(),
        }
    }

    fn convert_length_percentage(&self, val: UiVal) -> LengthPercentage {
        match val {
            UiVal::Px(v) => length(v),
            UiVal::Percent(v) => percent(v / 100.0),
            UiVal::Auto => length(0.0),
        }
    }

    fn convert_length_percentage_auto(&self, val: UiVal) -> LengthPercentageAuto {
        match val {
            UiVal::Px(v) => length(v),
            UiVal::Percent(v) => percent(v / 100.0),
            UiVal::Auto => auto(),
        }
    }
}
