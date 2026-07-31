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

//! Dock model — the tree of splits and tab groups behind the editor workbench.
//!
//! Pure data and geometry: no backend, no painting, no interaction. The shell
//! asks for a [`DockLayout`] and paints it; the widget layer turns pointer
//! events into calls back into [`DockTree`]. Keeping the model here means the
//! rules that are easy to get wrong — where a drop lands, what happens to a
//! split when its last tab closes, how ratios clamp — are testable without a
//! window.
//!
//! Chrome (title bar, spine, status bar) deliberately lives *outside* the dock:
//! those are the frame the workbench sits in, not panels the user rearranges.

use serde::{Deserialize, Serialize};

/// A rectangle as `[x, y, width, height]`, matching the convention used by
/// `UiBuilder` throughout the UI layer.
pub type DockRect = [f32; 4];

/// Which way a split divides its area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitAxis {
    /// Children sit side by side; the ratio splits the **width**.
    Horizontal,
    /// Children sit one above the other; the ratio splits the **height**.
    Vertical,
}

/// Where a dragged panel would land relative to the group under the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropZone {
    /// Join the group under the pointer as another tab.
    Center,
    /// Split it, taking the left half.
    Left,
    /// Split it, taking the right half.
    Right,
    /// Split it, taking the top half.
    Top,
    /// Split it, taking the bottom half.
    Bottom,
}

impl DropZone {
    /// The axis a drop in this zone splits along, or `None` for [`Center`],
    /// which joins an existing group instead of splitting.
    ///
    /// [`Center`]: DropZone::Center
    pub const fn axis(self) -> Option<SplitAxis> {
        match self {
            DropZone::Center => None,
            DropZone::Left | DropZone::Right => Some(SplitAxis::Horizontal),
            DropZone::Top | DropZone::Bottom => Some(SplitAxis::Vertical),
        }
    }

    /// Whether the dropped panel takes the **first** child of the new split.
    pub const fn takes_first(self) -> bool {
        matches!(self, DropZone::Left | DropZone::Top)
    }
}

/// Stable handle for a split, so a resize drag can name the divider it grabbed
/// without depending on the tree's shape surviving the gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SplitId(pub u32);

/// A node of the dock tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DockNode {
    /// A group of panels shown as tabs. Never empty — an emptied group is
    /// collapsed by [`DockTree::remove`] rather than left behind, because a
    /// zero-tab group would occupy space that has nothing to show.
    Tabs {
        /// Panel ids, in tab order.
        panels: Vec<String>,
        /// Index into `panels` of the visible one.
        active: usize,
    },
    /// Two children sharing the area along `axis`.
    Split {
        /// Stable handle for resize interaction.
        id: SplitId,
        /// Direction of the division.
        axis: SplitAxis,
        /// Fraction of the area given to `first`, clamped to a sane band.
        ratio: f32,
        /// Leading child — left or top.
        first: Box<DockNode>,
        /// Trailing child — right or bottom.
        second: Box<DockNode>,
    },
}

/// Thickness of a splitter's grab band, in points. Wide enough to hit without
/// aiming, narrow enough not to steal clicks from the panels either side.
pub const SPLITTER_THICKNESS: f32 = 6.0;

/// Smallest area a tab group may be squeezed to. Ratios clamp so a drag can
/// never collapse a panel to nothing — a panel you cannot see is a panel you
/// cannot get back without knowing the layout can be reset.
pub const MIN_PANE: f32 = 120.0;

/// One tab group placed on screen.
#[derive(Debug, Clone, PartialEq)]
pub struct TabGroupLayout {
    /// Area assigned to the whole group, tab strip included.
    pub rect: DockRect,
    /// Panel ids in tab order.
    pub panels: Vec<String>,
    /// Index of the visible panel.
    pub active: usize,
}

impl TabGroupLayout {
    /// Id of the panel whose content should be drawn.
    pub fn active_panel(&self) -> Option<&str> {
        self.panels.get(self.active).map(|s| s.as_str())
    }
}

/// One draggable divider placed on screen.
#[derive(Debug, Clone, PartialEq)]
pub struct SplitterLayout {
    /// Handle to pass back to [`DockTree::set_ratio`].
    pub id: SplitId,
    /// Grab band, already thickened to [`SPLITTER_THICKNESS`].
    pub rect: DockRect,
    /// Which way dragging it moves the boundary.
    pub axis: SplitAxis,
    /// Area the split governs — the caller needs it to turn a pointer position
    /// back into a ratio.
    pub bounds: DockRect,
}

/// Everything the shell needs to paint one frame of the dock.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DockLayout {
    /// Tab groups, in traversal order.
    pub groups: Vec<TabGroupLayout>,
    /// Dividers between them.
    pub splitters: Vec<SplitterLayout>,
}

/// A tree of splits and tab groups covering one workbench area.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DockTree {
    root: Option<DockNode>,
    /// Monotonic source of [`SplitId`]s. Never reused, so a handle held across
    /// a structural change can only go stale, never silently address a
    /// different divider.
    next_split: u32,
}

impl Default for DockTree {
    fn default() -> Self {
        Self::empty()
    }
}

impl DockTree {
    /// A dock with nothing in it.
    pub fn empty() -> Self {
        Self {
            root: None,
            next_split: 0,
        }
    }

    /// A dock holding a single panel.
    pub fn single(panel: impl Into<String>) -> Self {
        Self {
            root: Some(DockNode::Tabs {
                panels: vec![panel.into()],
                active: 0,
            }),
            next_split: 0,
        }
    }

    /// Whether the dock holds no panels at all.
    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    /// Every panel id in the tree, in traversal order.
    pub fn panels(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(root) = &self.root {
            collect_panels(root, &mut out);
        }
        out
    }

    /// Whether `panel` is somewhere in the tree.
    pub fn contains(&self, panel: &str) -> bool {
        self.panels().iter().any(|p| p == panel)
    }

    fn alloc_split(&mut self) -> SplitId {
        let id = SplitId(self.next_split);
        self.next_split += 1;
        id
    }

    /// Adds `panel` to the tree relative to `target`.
    ///
    /// `Center` joins `target`'s tab group; the edge zones split it. A panel
    /// already in the tree is moved rather than duplicated, which is what makes
    /// this the single entry point for a drag-and-drop rearrange.
    ///
    /// With no `target` — or a `target` that isn't in the tree — the panel
    /// splits the root, or becomes the root if the dock is empty.
    ///
    /// Returns the [`SplitId`] of the divider this created, so a caller
    /// building a default layout can set its ratio without having to guess at
    /// allocation order. `None` when the panel joined a tab group instead.
    pub fn insert(
        &mut self,
        panel: impl Into<String>,
        target: Option<&str>,
        zone: DropZone,
    ) -> Option<SplitId> {
        let panel = panel.into();
        self.remove(&panel);

        let Some(root) = self.root.take() else {
            self.root = Some(DockNode::Tabs {
                panels: vec![panel],
                active: 0,
            });
            return None;
        };

        let new_group = DockNode::Tabs {
            panels: vec![panel.clone()],
            active: 0,
        };

        // Resolve the target group first: after `remove` the tree may have
        // collapsed, and a target that no longer exists must fall back to the
        // root rather than silently drop the panel.
        let target_exists = target.is_some_and(|t| node_contains(&root, t));

        let split_id = self.alloc_split();
        self.root = Some(if target_exists {
            let target = target.expect("checked by target_exists");
            insert_at(root, target, new_group, zone, split_id)
        } else {
            match zone.axis() {
                None => {
                    // No target and no axis: join the first group we find, so a
                    // centre-drop onto empty space still places the panel.
                    push_into_first_group(root, &panel)
                }
                Some(axis) => make_split(split_id, axis, root, new_group, zone),
            }
        });

        // Every path that had an axis produced exactly one new split.
        zone.axis().map(|_| split_id)
    }

    /// Removes `panel`. An emptied tab group collapses, and its parent split is
    /// replaced by the surviving sibling so no blank area is left behind.
    ///
    /// Returns whether the panel was there.
    pub fn remove(&mut self, panel: &str) -> bool {
        let Some(root) = self.root.take() else {
            return false;
        };
        let (node, removed) = remove_from(root, panel);
        self.root = node;
        removed
    }

    /// Makes `panel` the visible tab of its group. No-op if it isn't in the
    /// tree.
    pub fn activate(&mut self, panel: &str) -> bool {
        self.root
            .as_mut()
            .is_some_and(|root| activate_in(root, panel))
    }

    /// Sets a split's ratio, clamped to `0.05..=0.95` so a divider dragged to
    /// the edge leaves the pane recoverable.
    pub fn set_ratio(&mut self, id: SplitId, ratio: f32) -> bool {
        self.root
            .as_mut()
            .is_some_and(|root| set_ratio_in(root, id, ratio.clamp(0.05, 0.95)))
    }

    /// Places every group and divider inside `area`.
    pub fn layout(&self, area: DockRect) -> DockLayout {
        let mut out = DockLayout::default();
        if let Some(root) = &self.root {
            layout_node(root, area, &mut out);
        }
        out
    }
}

// ── Free functions: the recursion, kept out of the impl so each rule reads on
//    its own ────────────────────────────────────────────────────────────────

fn collect_panels(node: &DockNode, out: &mut Vec<String>) {
    match node {
        DockNode::Tabs { panels, .. } => out.extend(panels.iter().cloned()),
        DockNode::Split { first, second, .. } => {
            collect_panels(first, out);
            collect_panels(second, out);
        }
    }
}

fn node_contains(node: &DockNode, panel: &str) -> bool {
    match node {
        DockNode::Tabs { panels, .. } => panels.iter().any(|p| p == panel),
        DockNode::Split { first, second, .. } => {
            node_contains(first, panel) || node_contains(second, panel)
        }
    }
}

fn make_split(
    id: SplitId,
    axis: SplitAxis,
    existing: DockNode,
    incoming: DockNode,
    zone: DropZone,
) -> DockNode {
    let (first, second) = if zone.takes_first() {
        (incoming, existing)
    } else {
        (existing, incoming)
    };
    DockNode::Split {
        id,
        axis,
        ratio: 0.5,
        first: Box::new(first),
        second: Box::new(second),
    }
}

/// Adds `panel` to the first tab group found, depth-first.
fn push_into_first_group(node: DockNode, panel: &str) -> DockNode {
    match node {
        DockNode::Tabs { mut panels, .. } => {
            panels.push(panel.to_owned());
            let active = panels.len() - 1;
            DockNode::Tabs { panels, active }
        }
        DockNode::Split {
            id,
            axis,
            ratio,
            first,
            second,
        } => DockNode::Split {
            id,
            axis,
            ratio,
            first: Box::new(push_into_first_group(*first, panel)),
            second,
        },
    }
}

fn insert_at(
    node: DockNode,
    target: &str,
    incoming: DockNode,
    zone: DropZone,
    split_id: SplitId,
) -> DockNode {
    match node {
        DockNode::Tabs { mut panels, active } => {
            if !panels.iter().any(|p| p == target) {
                return DockNode::Tabs { panels, active };
            }
            match zone.axis() {
                None => {
                    // Joining the group: the dropped panel becomes active, so
                    // the user sees what they just moved.
                    if let DockNode::Tabs {
                        panels: incoming_panels,
                        ..
                    } = incoming
                    {
                        panels.extend(incoming_panels);
                    }
                    let active = panels.len().saturating_sub(1);
                    DockNode::Tabs { panels, active }
                }
                Some(axis) => make_split(
                    split_id,
                    axis,
                    DockNode::Tabs { panels, active },
                    incoming,
                    zone,
                ),
            }
        }
        DockNode::Split {
            id,
            axis,
            ratio,
            first,
            second,
        } => {
            if node_contains(&first, target) {
                DockNode::Split {
                    id,
                    axis,
                    ratio,
                    first: Box::new(insert_at(*first, target, incoming, zone, split_id)),
                    second,
                }
            } else {
                DockNode::Split {
                    id,
                    axis,
                    ratio,
                    first,
                    second: Box::new(insert_at(*second, target, incoming, zone, split_id)),
                }
            }
        }
    }
}

fn remove_from(node: DockNode, panel: &str) -> (Option<DockNode>, bool) {
    match node {
        DockNode::Tabs { mut panels, active } => {
            let Some(pos) = panels.iter().position(|p| p == panel) else {
                return (Some(DockNode::Tabs { panels, active }), false);
            };
            panels.remove(pos);
            if panels.is_empty() {
                return (None, true);
            }
            // Keep the neighbour selected rather than snapping to the first
            // tab: closing one of several tabs should leave you next to where
            // you were.
            let active = active.min(panels.len() - 1);
            (Some(DockNode::Tabs { panels, active }), true)
        }
        DockNode::Split {
            id,
            axis,
            ratio,
            first,
            second,
        } => {
            let (first, removed_first) = remove_from(*first, panel);
            let (second, removed) = if removed_first {
                (Some(*second), true)
            } else {
                let (s, r) = remove_from(*second, panel);
                (s, r)
            };
            match (first, second) {
                (Some(f), Some(s)) => (
                    Some(DockNode::Split {
                        id,
                        axis,
                        ratio,
                        first: Box::new(f),
                        second: Box::new(s),
                    }),
                    removed,
                ),
                // A split with one surviving child is not a split any more.
                (Some(only), None) | (None, Some(only)) => (Some(only), removed),
                (None, None) => (None, removed),
            }
        }
    }
}

fn activate_in(node: &mut DockNode, panel: &str) -> bool {
    match node {
        DockNode::Tabs { panels, active } => {
            if let Some(pos) = panels.iter().position(|p| p == panel) {
                *active = pos;
                true
            } else {
                false
            }
        }
        DockNode::Split { first, second, .. } => {
            activate_in(first, panel) || activate_in(second, panel)
        }
    }
}

fn set_ratio_in(node: &mut DockNode, target: SplitId, value: f32) -> bool {
    match node {
        DockNode::Tabs { .. } => false,
        DockNode::Split {
            id,
            ratio,
            first,
            second,
            ..
        } => {
            if *id == target {
                *ratio = value;
                return true;
            }
            set_ratio_in(first, target, value) || set_ratio_in(second, target, value)
        }
    }
}

fn layout_node(node: &DockNode, area: DockRect, out: &mut DockLayout) {
    match node {
        DockNode::Tabs { panels, active } => out.groups.push(TabGroupLayout {
            rect: area,
            panels: panels.clone(),
            active: *active,
        }),
        DockNode::Split {
            id,
            axis,
            ratio,
            first,
            second,
        } => {
            let [x, y, w, h] = area;
            let half = SPLITTER_THICKNESS * 0.5;
            match axis {
                SplitAxis::Horizontal => {
                    // Clamp in pixels, not in ratio: the same ratio means a
                    // different width at every window size, so a ratio-only
                    // clamp cannot promise a usable pane.
                    let cut = (w * ratio).clamp(MIN_PANE.min(w * 0.5), (w - MIN_PANE).max(w * 0.5));
                    layout_node(first, [x, y, (cut - half).max(0.0), h], out);
                    layout_node(
                        second,
                        [x + cut + half, y, (w - cut - half).max(0.0), h],
                        out,
                    );
                    out.splitters.push(SplitterLayout {
                        id: *id,
                        rect: [x + cut - half, y, SPLITTER_THICKNESS, h],
                        axis: *axis,
                        bounds: area,
                    });
                }
                SplitAxis::Vertical => {
                    let cut = (h * ratio).clamp(MIN_PANE.min(h * 0.5), (h - MIN_PANE).max(h * 0.5));
                    layout_node(first, [x, y, w, (cut - half).max(0.0)], out);
                    layout_node(
                        second,
                        [x, y + cut + half, w, (h - cut - half).max(0.0)],
                        out,
                    );
                    out.splitters.push(SplitterLayout {
                        id: *id,
                        rect: [x, y + cut - half, w, SPLITTER_THICKNESS],
                        axis: *axis,
                        bounds: area,
                    });
                }
            }
        }
    }
}

/// Fraction of the shorter side taken by each edge drop band.
///
/// Kept under `0.146`, the point where the four bands would together claim half
/// the area: joining a tab group is the common intent and must stay the easy
/// target, while a split is the deliberate one and only needs a band wide
/// enough to hit without aiming (48 px on a 400 px pane).
const EDGE_BAND: f32 = 0.12;

/// Classifies a pointer position inside `rect` into a drop zone.
///
/// The centre keeps the majority of the area on purpose: joining a tab group is
/// the common intent, and a split is the deliberate one. Bands are measured
/// against the shorter side so a long thin panel doesn't turn almost entirely
/// into edge.
pub fn zone_at(rect: DockRect, pointer: [f32; 2]) -> Option<DropZone> {
    let [x, y, w, h] = rect;
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let (px, py) = (pointer[0] - x, pointer[1] - y);
    if px < 0.0 || py < 0.0 || px > w || py > h {
        return None;
    }

    let band = (w.min(h) * EDGE_BAND).max(1.0);
    // Distance to each edge; the smallest wins, so corners resolve to whichever
    // edge the pointer is genuinely nearer rather than to a fixed precedence.
    let (left, right, top, bottom) = (px, w - px, py, h - py);
    let nearest = left.min(right).min(top).min(bottom);
    if nearest > band {
        return Some(DropZone::Center);
    }
    Some(if nearest == left {
        DropZone::Left
    } else if nearest == right {
        DropZone::Right
    } else if nearest == top {
        DropZone::Top
    } else {
        DropZone::Bottom
    })
}

/// Turns a pointer position on a splitter into the ratio it implies.
///
/// Lives next to [`DockTree::set_ratio`] so the geometry the layout used and
/// the geometry the drag inverts cannot drift apart.
pub fn ratio_from_pointer(splitter: &SplitterLayout, pointer: [f32; 2]) -> f32 {
    let [bx, by, bw, bh] = splitter.bounds;
    let raw = match splitter.axis {
        SplitAxis::Horizontal if bw > 0.0 => (pointer[0] - bx) / bw,
        SplitAxis::Vertical if bh > 0.0 => (pointer[1] - by) / bh,
        _ => 0.5,
    };
    raw.clamp(0.05, 0.95)
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: DockRect = [0.0, 0.0, 1000.0, 800.0];

    fn tree_of(panels: &[&str]) -> DockTree {
        let mut t = DockTree::single(panels[0]);
        for p in &panels[1..] {
            t.insert(*p, Some(panels[0]), DropZone::Center);
        }
        t
    }

    #[test]
    fn single_panel_fills_the_area() {
        let t = DockTree::single("viewport");
        let l = t.layout(AREA);
        assert_eq!(l.groups.len(), 1);
        assert_eq!(l.groups[0].rect, AREA);
        assert!(l.splitters.is_empty(), "one group needs no divider");
    }

    /// A centre drop joins the group; an edge drop splits it. This is the whole
    /// interaction in one assertion pair.
    #[test]
    fn centre_joins_and_edge_splits() {
        let mut joined = DockTree::single("a");
        joined.insert("b", Some("a"), DropZone::Center);
        assert_eq!(joined.layout(AREA).groups.len(), 1);

        let mut split = DockTree::single("a");
        split.insert("b", Some("a"), DropZone::Right);
        let l = split.layout(AREA);
        assert_eq!(l.groups.len(), 2);
        assert_eq!(l.splitters.len(), 1);
    }

    /// `Left`/`Top` put the dropped panel first, `Right`/`Bottom` put it second
    /// — otherwise a drop lands on the opposite side from the one shown.
    #[test]
    fn drop_side_decides_order() {
        let mut left = DockTree::single("a");
        left.insert("b", Some("a"), DropZone::Left);
        let l = left.layout(AREA);
        assert_eq!(l.groups[0].panels, vec!["b"]);
        assert_eq!(l.groups[1].panels, vec!["a"]);

        let mut bottom = DockTree::single("a");
        bottom.insert("b", Some("a"), DropZone::Bottom);
        let l = bottom.layout(AREA);
        assert_eq!(l.groups[0].panels, vec!["a"]);
        assert_eq!(l.groups[1].panels, vec!["b"]);
        assert_eq!(l.splitters[0].axis, SplitAxis::Vertical);
    }

    /// Re-inserting a panel moves it. Without the implicit remove, dragging a
    /// tab elsewhere would leave a copy behind.
    #[test]
    fn insert_moves_rather_than_duplicates() {
        let mut t = tree_of(&["a", "b", "c"]);
        t.insert("b", Some("a"), DropZone::Right);
        let panels = t.panels();
        assert_eq!(panels.iter().filter(|p| *p == "b").count(), 1);
        assert_eq!(panels.len(), 3);
    }

    /// Emptying a group collapses its split, so no blank area survives.
    #[test]
    fn removing_last_tab_collapses_the_split() {
        let mut t = DockTree::single("a");
        t.insert("b", Some("a"), DropZone::Right);
        assert_eq!(t.layout(AREA).splitters.len(), 1);

        assert!(t.remove("b"));
        let l = t.layout(AREA);
        assert_eq!(l.groups.len(), 1);
        assert!(l.splitters.is_empty(), "the divider must go with the pane");
        assert_eq!(l.groups[0].rect, AREA, "the survivor takes the whole area");
    }

    #[test]
    fn removing_the_only_panel_empties_the_dock() {
        let mut t = DockTree::single("a");
        assert!(t.remove("a"));
        assert!(t.is_empty());
        assert!(t.layout(AREA).groups.is_empty());
        assert!(!t.remove("a"), "removing twice reports nothing removed");
    }

    /// Closing a tab keeps the neighbour selected instead of snapping to the
    /// first one.
    #[test]
    fn closing_a_tab_keeps_a_valid_neighbour_active() {
        let mut t = tree_of(&["a", "b", "c"]);
        t.activate("c");
        assert!(t.remove("c"));
        let g = &t.layout(AREA).groups[0];
        assert!(g.active < g.panels.len(), "active index must stay in range");
        assert_eq!(g.active_panel(), Some("b"));
    }

    #[test]
    fn ratio_clamps_so_a_pane_never_disappears() {
        let mut t = DockTree::single("a");
        t.insert("b", Some("a"), DropZone::Right);
        let id = t.layout(AREA).splitters[0].id;

        t.set_ratio(id, -5.0);
        let l = t.layout(AREA);
        assert!(l.groups[0].rect[2] >= MIN_PANE * 0.5, "left pane survives");

        t.set_ratio(id, 99.0);
        let l = t.layout(AREA);
        assert!(l.groups[1].rect[2] >= MIN_PANE * 0.5, "right pane survives");
    }

    /// Splitting twice nests, and both dividers stay addressable — a resize
    /// must not be able to grab the wrong one.
    #[test]
    fn nested_splits_keep_distinct_ids() {
        let mut t = DockTree::single("a");
        t.insert("b", Some("a"), DropZone::Right);
        t.insert("c", Some("b"), DropZone::Bottom);
        let l = t.layout(AREA);
        assert_eq!(l.groups.len(), 3);
        assert_eq!(l.splitters.len(), 2);
        assert_ne!(l.splitters[0].id, l.splitters[1].id);
    }

    /// Panes tile the area without overlapping — the property that makes the
    /// layout usable at all.
    #[test]
    fn panes_do_not_overlap() {
        let mut t = DockTree::single("a");
        t.insert("b", Some("a"), DropZone::Right);
        t.insert("c", Some("b"), DropZone::Bottom);
        let groups = t.layout(AREA).groups;
        for (i, g) in groups.iter().enumerate() {
            for other in &groups[i + 1..] {
                let sep_x = g.rect[0] + g.rect[2] <= other.rect[0] + 0.01
                    || other.rect[0] + other.rect[2] <= g.rect[0] + 0.01;
                let sep_y = g.rect[1] + g.rect[3] <= other.rect[1] + 0.01
                    || other.rect[1] + other.rect[3] <= g.rect[1] + 0.01;
                assert!(sep_x || sep_y, "{:?} overlaps {:?}", g.rect, other.rect);
            }
        }
    }

    #[test]
    fn zone_at_reads_the_centre_and_the_four_edges() {
        let r = [0.0, 0.0, 400.0, 400.0];
        assert_eq!(zone_at(r, [200.0, 200.0]), Some(DropZone::Center));
        assert_eq!(zone_at(r, [5.0, 200.0]), Some(DropZone::Left));
        assert_eq!(zone_at(r, [395.0, 200.0]), Some(DropZone::Right));
        assert_eq!(zone_at(r, [200.0, 5.0]), Some(DropZone::Top));
        assert_eq!(zone_at(r, [200.0, 395.0]), Some(DropZone::Bottom));
        assert_eq!(zone_at(r, [-1.0, 200.0]), None, "outside is not a zone");
    }

    /// The centre keeps most of the area: joining a group is the common
    /// intent, splitting is the deliberate one.
    #[test]
    fn centre_zone_dominates_the_area() {
        let r = [0.0, 0.0, 400.0, 400.0];
        let mut centre = 0;
        let mut total = 0;
        for gx in 0..40 {
            for gy in 0..40 {
                total += 1;
                if zone_at(r, [gx as f32 * 10.0 + 5.0, gy as f32 * 10.0 + 5.0])
                    == Some(DropZone::Center)
                {
                    centre += 1;
                }
            }
        }
        assert!(
            centre * 2 > total,
            "centre should own the majority ({centre}/{total})"
        );
    }

    /// A drag inverts the same geometry the layout produced.
    #[test]
    fn ratio_from_pointer_inverts_the_layout() {
        let mut t = DockTree::single("a");
        t.insert("b", Some("a"), DropZone::Right);
        let sp = t.layout(AREA).splitters[0].clone();
        let ratio = ratio_from_pointer(&sp, [250.0, 400.0]);
        assert!((ratio - 0.25).abs() < 1e-4, "got {ratio}");

        t.set_ratio(sp.id, ratio);
        let l = t.layout(AREA);
        assert!(
            (l.groups[0].rect[2] - 247.0).abs() < 4.0,
            "left pane follows"
        );
    }

    /// A target that vanished (its group collapsed as part of the same move)
    /// must not swallow the panel.
    #[test]
    fn insert_with_unknown_target_still_places_the_panel() {
        let mut t = DockTree::single("a");
        t.insert("b", Some("ghost"), DropZone::Right);
        assert!(t.contains("b"));
        assert_eq!(t.panels().len(), 2);

        let mut t2 = DockTree::single("a");
        t2.insert("b", None, DropZone::Center);
        assert!(t2.contains("b"), "a centre drop with no target still lands");
    }

    /// The tree round-trips through RON, which is what layout persistence will
    /// rely on.
    #[test]
    fn tree_round_trips_through_serde() {
        let mut t = DockTree::single("a");
        t.insert("b", Some("a"), DropZone::Right);
        t.insert("c", Some("b"), DropZone::Bottom);
        t.set_ratio(t.layout(AREA).splitters[0].id, 0.3);

        let encoded = serde_json::to_string(&t).expect("serialize");
        let decoded: DockTree = serde_json::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded, t);
        assert_eq!(decoded.layout(AREA), t.layout(AREA));
    }
}
