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

//! The Taffy [`LayoutSystem`]: lays out every entity with a `UiNode` and
//! writes its `UiTransform`.

use khora_core::ecs::entity::EntityId;
use khora_core::math::Vec2;
use khora_core::ui::layout::UiLayoutView;
use khora_core::ui::types::{UiFlexDirection, UiNode, UiTransform, UiVal};
use khora_core::ui::LayoutSystem;
use std::any::Any;
use std::collections::HashMap;
use taffy::prelude::*;

/// The UI layout system over Taffy.
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
    /// Creates a new `TaffyLayoutSystem`.
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

        // The trait's default size when the view has no live surface.
        let (viewport_w, viewport_h) = view.viewport_size();

        let entities = view.get_all_ui_entities();
        for entity in entities.iter() {
            if let Some(ui_node) = view.get_node(*entity) {
                let style = self.convert_style(&ui_node);
                match self.taffy.new_leaf(style) {
                    Ok(node) => {
                        self.entity_to_node.insert(*entity, node);
                    }
                    Err(e) => {
                        log::error!("TaffyLayoutSystem: failed to create layout node: {e}");
                    }
                }
            }
        }

        let mut roots = Vec::new();

        for &entity in entities.iter() {
            if view.has_parent(entity) {
                continue;
            }
            roots.push(entity);
            self.attach_children(entity, view);
        }

        // Compute layout against the real viewport.
        for root in roots {
            if let Some(&root_node) = self.entity_to_node.get(&root) {
                let available_space = Size {
                    width: AvailableSpace::Definite(viewport_w as f32),
                    height: AvailableSpace::Definite(viewport_h as f32),
                };

                if let Err(e) = self.taffy.compute_layout(root_node, available_space) {
                    log::error!("TaffyLayoutSystem: compute_layout failed: {e}");
                    continue;
                }
                // Taffy leaves a root at the origin and keeps its margin aside.
                let origin = match self.taffy.layout(root_node) {
                    Ok(layout) => Vec2::new(layout.margin.left, layout.margin.top),
                    Err(_) => Vec2::new(0.0, 0.0),
                };
                self.update_transforms(root, view, 0, origin);
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

// SAFETY: `TaffyTree` is `!Send`/`!Sync` only because a `CompactLength` may
// carry a `*const ()`, the pointer of a `calc()` length, set by
// `CompactLength::calc` alone. This system never builds one (`convert_style`
// maps every value to points, a percent or auto), so every length in the tree
// is a tagged number and no pointer is ever moved or shared. The tree owns all
// its data, and is changed only through `&mut self`.
unsafe impl Send for TaffyLayoutSystem {}
// SAFETY: as for `Send` above.
unsafe impl Sync for TaffyLayoutSystem {}

impl TaffyLayoutSystem {
    fn attach_children(&mut self, entity: EntityId, view: &dyn UiLayoutView) {
        let children = view.get_children(entity);
        if !children.is_empty() {
            let mut taffy_children = Vec::new();
            for &child_id in &children {
                if let Some(&child_node) = self.entity_to_node.get(&child_id) {
                    self.attach_children(child_id, view);
                    taffy_children.push(child_node);
                }
            }

            if !taffy_children.is_empty() {
                if let Some(&parent_node) = self.entity_to_node.get(&entity) {
                    if let Err(e) = self.taffy.set_children(parent_node, &taffy_children) {
                        log::error!("TaffyLayoutSystem: failed to set children: {e}");
                    }
                }
            }
        }
    }

    /// Writes `entity`'s screen-space transform, `origin` being its parent's
    /// screen position, then its children's.
    fn update_transforms(
        &self,
        entity: EntityId,
        view: &mut dyn UiLayoutView,
        z_index: i32,
        origin: Vec2,
    ) {
        let mut pos = origin;
        if let Some(&node_id) = self.entity_to_node.get(&entity) {
            if let Ok(layout) = self.taffy.layout(node_id) {
                // Taffy places a node relative to its parent.
                pos = origin + Vec2::new(layout.location.x, layout.location.y);
                let transform = UiTransform {
                    pos,
                    size: Vec2::new(layout.size.width, layout.size.height),
                    z_index,
                };
                view.set_transform(entity, transform);
            }
        }

        let children = view.get_children(entity);
        for child_id in children {
            self.update_transforms(child_id, view, z_index + 1, pos);
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
            UiVal::Auto => length(0.0_f32),
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

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::ui::types::UiRect;

    /// A UI tree held in memory: its nodes in order, each parent's children,
    /// and the transforms the layout wrote back.
    #[derive(Default)]
    struct Tree {
        nodes: Vec<(EntityId, UiNode)>,
        children: HashMap<EntityId, Vec<EntityId>>,
        transforms: HashMap<EntityId, UiTransform>,
    }

    impl Tree {
        /// Adds `node` as entity `index`, under `parent` if any.
        fn add(&mut self, index: u32, node: UiNode, parent: Option<EntityId>) -> EntityId {
            let entity = EntityId {
                index,
                generation: 0,
            };
            self.nodes.push((entity, node));
            if let Some(parent) = parent {
                self.children.entry(parent).or_default().push(entity);
            }
            entity
        }

        /// The position the layout wrote for `entity`.
        fn pos(&self, entity: EntityId) -> Vec2 {
            self.transforms
                .get(&entity)
                .unwrap_or_else(|| panic!("no transform was written for {entity:?}"))
                .pos
        }
    }

    impl UiLayoutView for Tree {
        fn get_all_ui_entities(&self) -> Vec<EntityId> {
            self.nodes.iter().map(|(entity, _)| *entity).collect()
        }
        fn get_node(&self, entity: EntityId) -> Option<UiNode> {
            self.nodes
                .iter()
                .find(|(e, _)| *e == entity)
                .map(|(_, node)| node.clone())
        }
        fn get_children(&self, entity: EntityId) -> Vec<EntityId> {
            self.children.get(&entity).cloned().unwrap_or_default()
        }
        fn has_parent(&self, entity: EntityId) -> bool {
            self.children.values().any(|kids| kids.contains(&entity))
        }
        fn set_transform(&mut self, entity: EntityId, transform: UiTransform) {
            self.transforms.insert(entity, transform);
        }
    }

    /// A node of `width` × `height` pixels, with `padding` on every side and
    /// no margin.
    ///
    /// The default margin is `Auto`, which would centre the node in its parent.
    fn sized(width: f32, height: f32, padding: f32) -> UiNode {
        UiNode {
            width: UiVal::Px(width),
            height: UiVal::Px(height),
            padding: UiRect::all(UiVal::Px(padding)),
            margin: UiRect::all(UiVal::Px(0.0)),
            ..Default::default()
        }
    }

    /// **A nested node lands at its ancestors' positions plus its own offset.**
    ///
    /// A 200 × 200 root at the origin with a padding of 20 stacks two children
    /// in a column: `first` (100 × 50) at (20, 20), `second` (100 × 60, padding
    /// 10) below it at (20, 70). `second` holds `inner` (40 × 40, left margin
    /// 5), offset (15, 10) inside it: on screen `inner` is at (35, 80), not at
    /// its offset from its parent.
    #[test]
    fn a_nested_node_is_placed_at_its_parents_position_plus_its_own_offset() {
        let mut tree = Tree::default();
        let root = tree.add(0, sized(200.0, 200.0, 20.0), None);
        let first = tree.add(1, sized(100.0, 50.0, 0.0), Some(root));
        let second = tree.add(2, sized(100.0, 60.0, 10.0), Some(root));
        let inner = tree.add(
            3,
            UiNode {
                margin: UiRect {
                    left: UiVal::Px(5.0),
                    right: UiVal::Px(0.0),
                    top: UiVal::Px(0.0),
                    bottom: UiVal::Px(0.0),
                },
                ..sized(40.0, 40.0, 0.0)
            },
            Some(second),
        );

        TaffyLayoutSystem::new().compute_layouts(&mut tree);

        assert_eq!(tree.pos(root), Vec2::new(0.0, 0.0), "the root");
        assert_eq!(
            tree.pos(first),
            Vec2::new(20.0, 20.0),
            "the first child, inside the root's padding"
        );
        assert_eq!(
            tree.pos(second),
            Vec2::new(20.0, 70.0),
            "the second child, below the first"
        );
        assert_eq!(
            tree.pos(inner),
            Vec2::new(35.0, 80.0),
            "the grandchild: its parent's (20, 70) plus its own (15, 10)"
        );
    }

    /// **A root's margin moves it, and its children, away from the screen's corner.**
    ///
    /// A 200 × 200 root with a margin of 10 on every side holds a 50 × 50
    /// child: on screen the root's top-left corner is at (10, 10), and the
    /// child, at the start of the root's content, is there too.
    #[test]
    fn a_root_with_a_margin_is_placed_past_its_margin() {
        let mut tree = Tree::default();
        let root = tree.add(
            0,
            UiNode {
                margin: UiRect::all(UiVal::Px(10.0)),
                ..sized(200.0, 200.0, 0.0)
            },
            None,
        );
        let child = tree.add(1, sized(50.0, 50.0, 0.0), Some(root));

        TaffyLayoutSystem::new().compute_layouts(&mut tree);

        assert_eq!(tree.pos(root), Vec2::new(10.0, 10.0), "the root");
        assert_eq!(tree.pos(child), Vec2::new(10.0, 10.0), "its child");
    }
}
