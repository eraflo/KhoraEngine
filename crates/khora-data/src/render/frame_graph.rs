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

//! Per-frame render graph: agents enqueue declarative passes here, the engine
//! drains the graph after the scheduler completes and submits the recorded
//! command buffers in topological order.
//!
//! Lives in `khora-data` because it is a *data container* — there is no
//! GORNA strategy to negotiate, no behavior to swap. The submission step
//! (`submit_frame_graph`) is invoked directly by `EngineCore`.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use khora_core::renderer::api::command::CommandBufferId;
use khora_core::renderer::GraphicsDevice;

/// Logical resource handle in the frame graph — used for declaring reads/writes.
///
/// Resources here are *abstract*: the actual GPU views live in `FrameContext`
/// (`ColorTarget`, `DepthTarget`, `ShadowAtlasView`). The graph only orders
/// passes; it does not allocate or alias resources yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceId {
    /// Swapchain (or offscreen viewport) color attachment.
    Color,
    /// Frame depth attachment.
    Depth,
    /// Shadow atlas — written by `ShadowAgent`, read by `RenderAgent`.
    ShadowAtlas,
    /// Custom resource keyed by an opaque integer (plugin / future use).
    Custom(u64),
}

/// Declarative description of a render pass.
#[derive(Debug, Clone)]
pub struct PassDescriptor {
    /// Debug-friendly pass name.
    pub name: &'static str,
    /// Resources read by this pass.
    pub reads: Vec<ResourceId>,
    /// Resources written by this pass.
    pub writes: Vec<ResourceId>,
}

impl PassDescriptor {
    /// Builds a descriptor with the given name and no declared resources.
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            reads: Vec::new(),
            writes: Vec::new(),
        }
    }

    /// Adds a read dependency.
    pub fn reads(mut self, id: ResourceId) -> Self {
        self.reads.push(id);
        self
    }

    /// Adds a write dependency.
    pub fn writes(mut self, id: ResourceId) -> Self {
        self.writes.push(id);
        self
    }
}

/// One pre-recorded pass: descriptor + finished command buffer.
struct RecordedPass {
    descriptor: PassDescriptor,
    command_buffer: CommandBufferId,
}

/// A render pass an agent contributes for this frame.
///
/// Instead of locking the shared [`SharedFrameGraph`] to `add_pass` directly,
/// a recording agent writes its contribution into a per-layer slot on its
/// [`OutputDeck`](khora_core::lane::OutputDeck) ([`ScenePassSlot`],
/// [`UiPassSlot`], [`GridPassSlot`], …). The engine folds those slots into the
/// graph in their registered order (scene → ui → overlays) after the agent
/// wave —
/// preserving the previous insertion order while freeing the agents from the
/// `Mutex<FrameGraph>`, so they can run concurrently.
pub struct PassContribution {
    /// The pass's resource read/write declaration.
    pub descriptor: PassDescriptor,
    /// The finished command buffer recorded for the pass.
    pub command_buffer: CommandBufferId,
}

/// Deck slot carrying the main scene pass, written by `RenderAgent`.
#[derive(Default)]
pub struct ScenePassSlot(pub Option<PassContribution>);

/// Deck slot carrying the UI pass, written by `UiAgent`.
#[derive(Default)]
pub struct UiPassSlot(pub Option<PassContribution>);

/// Deck slot carrying the skybox / environment-background pass, written by
/// the skybox family. Folded in after the scene pass and before overlays, so the
/// sky sits behind the geometry (depth-tested) and under the debug overlays.
#[derive(Default)]
pub struct SkyboxPassSlot(pub Option<PassContribution>);

impl khora_core::lane::io::PassSlot for SkyboxPassSlot {
    type Descriptor = PassDescriptor;

    /// The sky paints the colour behind what the scene's depth holds.
    fn descriptor() -> PassDescriptor {
        PassDescriptor::new("SkyboxPass")
            .writes(ResourceId::Color)
            .reads(ResourceId::Depth)
    }

    fn close(descriptor: PassDescriptor, command_buffer: CommandBufferId) -> Self {
        Self(Some(PassContribution {
            descriptor,
            command_buffer,
        }))
    }
}

/// Deck slot carrying the scene's **transparent** draws, written by
/// `RenderAgent` alongside [`ScenePassSlot`].
///
/// # Why the scene is two passes
///
/// A blended surface must not write depth — two transparent surfaces could not
/// composite if it did. So it leaves the pixel at the far plane, and the skybox
/// paints exactly the pixels left at the far plane. With the whole scene in one
/// pass, the sky therefore repainted the glass wherever there was no opaque
/// geometry behind it: a transparent sphere against the sky simply vanished,
/// while the half of it overlapping the floor survived.
///
/// Splitting the scene puts the sky where it belongs — **between** the opaque
/// draws that give it a depth buffer to test against and the blended draws that
/// composite over it.
///
/// The alternative was to draw the sky first and let it clear the target. That
/// would have made a background the frame cannot be rendered without: the
/// skybox agent is `Important`, not `Critical`, and SAA is built so that
/// dropping work costs quality, never correctness. With the split, an absent
/// sky costs you the sky — the opaque pass still clears, and the glass still
/// blends over the clear colour.
#[derive(Default)]
pub struct TransparentPassSlot(pub Option<PassContribution>);

/// Deck slot carrying the editor ground grid's pass, written by the grid
/// family. Submitted after the UI.
#[derive(Default)]
pub struct GridPassSlot(pub Option<PassContribution>);

impl khora_core::lane::io::PassSlot for GridPassSlot {
    type Descriptor = PassDescriptor;

    /// The grid paints the colour, depth-tested against the scene.
    fn descriptor() -> PassDescriptor {
        PassDescriptor::new("GridPass")
            .writes(ResourceId::Color)
            .reads(ResourceId::Depth)
    }

    fn close(descriptor: PassDescriptor, command_buffer: CommandBufferId) -> Self {
        Self(Some(PassContribution {
            descriptor,
            command_buffer,
        }))
    }
}

/// Deck slot carrying the wireframe overlay's pass, written by the wireframe
/// family. Submitted after the grid.
#[derive(Default)]
pub struct WireframePassSlot(pub Option<PassContribution>);

impl khora_core::lane::io::PassSlot for WireframePassSlot {
    type Descriptor = PassDescriptor;

    /// The wireframe paints the colour, depth-tested against the scene.
    fn descriptor() -> PassDescriptor {
        PassDescriptor::new("WireframePass")
            .writes(ResourceId::Color)
            .reads(ResourceId::Depth)
    }

    fn close(descriptor: PassDescriptor, command_buffer: CommandBufferId) -> Self {
        Self(Some(PassContribution {
            descriptor,
            command_buffer,
        }))
    }
}

/// Deck slot carrying the line gizmos' pass, written by the gizmo family.
/// Submitted after the wireframe, last of all.
#[derive(Default)]
pub struct GizmoPassSlot(pub Option<PassContribution>);

impl khora_core::lane::io::PassSlot for GizmoPassSlot {
    type Descriptor = PassDescriptor;

    /// The gizmos paint the colour on top of everything: no depth.
    fn descriptor() -> PassDescriptor {
        PassDescriptor::new("GizmoPass").writes(ResourceId::Color)
    }

    fn close(descriptor: PassDescriptor, command_buffer: CommandBufferId) -> Self {
        Self(Some(PassContribution {
            descriptor,
            command_buffer,
        }))
    }
}

/// Deck slot carrying the shadow-map pass, written by `ShadowAgent`.
///
/// Submitted first: every lit pass samples the shadow atlas it fills.
#[derive(Default)]
pub struct ShadowPassSlot(pub Option<PassContribution>);

impl khora_core::lane::io::PassSlot for ShadowPassSlot {
    type Descriptor = PassDescriptor;

    /// The shadow pass writes the shadow atlas, which every lit pass reads.
    fn descriptor() -> PassDescriptor {
        PassDescriptor::new("ShadowPass").writes(ResourceId::ShadowAtlas)
    }

    fn close(descriptor: PassDescriptor, command_buffer: CommandBufferId) -> Self {
        Self(Some(PassContribution {
            descriptor,
            command_buffer,
        }))
    }
}

/// One pass slot the engine submits each frame, and what it must follow.
///
/// Submitted by `register_pass!(Slot)` or `register_pass!(Slot, after = [A, B])`
/// through `inventory`; the SDK's submit stage takes every registered slot from
/// the frame's deck in [`registered_pass_order`].
#[derive(Debug)]
pub struct PassRegistration {
    /// The slot's name, as an error names it.
    pub name: &'static str,
    /// The slot's type.
    pub type_id: fn() -> std::any::TypeId,
    /// The slots this one is submitted after.
    pub after: &'static [fn() -> std::any::TypeId],
    /// Takes this frame's contribution out of the slot.
    pub take: fn(&mut khora_core::lane::OutputDeck) -> Option<PassContribution>,
}

inventory::collect!(PassRegistration);

/// The `after` edges of some pass slots make a cycle: no order submits each
/// after the ones it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassOrderError {
    /// The names of the slots on the cycle.
    pub cycle: Vec<&'static str>,
}

impl std::fmt::Display for PassOrderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the pass slots' `after` edges make a cycle: {} -> {}",
            self.cycle.join(" -> "),
            self.cycle.first().copied().unwrap_or_default()
        )
    }
}

impl std::error::Error for PassOrderError {}

/// Orders `passes` so each comes after every slot its `after` names.
///
/// A slot named in `after` but absent from `passes` orders nothing. Refuses a
/// cycle, naming the slots on it.
///
/// Slots no edge orders against each other are taken by name, so the order
/// does not depend on the order the linker happened to collect them in.
pub fn pass_order<'a>(
    passes: &[&'a PassRegistration],
) -> Result<Vec<&'a PassRegistration>, PassOrderError> {
    let ids: Vec<std::any::TypeId> = passes.iter().map(|pass| (pass.type_id)()).collect();
    // `before[i]`: the indices of the present slots pass `i` must follow.
    let before: Vec<Vec<usize>> = passes
        .iter()
        .map(|pass| {
            pass.after
                .iter()
                .filter_map(|after| ids.iter().position(|id| *id == after()))
                .collect()
        })
        .collect();

    let mut placed = vec![false; passes.len()];
    let mut order = Vec::with_capacity(passes.len());
    while order.len() < passes.len() {
        let ready = (0..passes.len())
            .filter(|&i| !placed[i] && before[i].iter().all(|&b| placed[b]))
            .min_by_key(|&i| passes[i].name);
        match ready {
            Some(i) => {
                placed[i] = true;
                order.push(passes[i]);
            }
            None => {
                return Err(PassOrderError {
                    cycle: find_cycle(passes, &before, &placed),
                })
            }
        }
    }
    Ok(order)
}

/// Walks the `after` edges among the slots left unplaced — every one of them
/// waits on another unplaced slot — until a slot repeats; the walk from its
/// first visit is the cycle.
fn find_cycle(
    passes: &[&PassRegistration],
    before: &[Vec<usize>],
    placed: &[bool],
) -> Vec<&'static str> {
    let Some(mut at) = (0..passes.len()).find(|&i| !placed[i]) else {
        return Vec::new();
    };
    let mut path: Vec<usize> = Vec::new();
    loop {
        if let Some(start) = path.iter().position(|&i| i == at) {
            return path[start..].iter().map(|&i| passes[i].name).collect();
        }
        path.push(at);
        match before[at].iter().find(|&&b| !placed[b]) {
            Some(&next) => at = next,
            None => return path.iter().map(|&i| passes[i].name).collect(),
        }
    }
}

/// [`pass_order`] over every [`PassRegistration`] linked into the binary: the
/// order the engine submits its passes in, checked once at boot.
pub fn registered_pass_order() -> Result<Vec<&'static PassRegistration>, PassOrderError> {
    let passes: Vec<&'static PassRegistration> = inventory::iter::<PassRegistration>().collect();
    pass_order(&passes)
}

/// Registers a pass slot the engine submits each frame, after the slots it
/// names.
///
/// ```ignore
/// register_pass!(ShadowPassSlot);
/// register_pass!(ScenePassSlot, after = [ShadowPassSlot]);
/// ```
///
/// The slot must be a `Default` deck value holding an
/// `Option<PassContribution>` in its first field.
#[macro_export]
macro_rules! register_pass {
    ($slot:ty) => {
        $crate::register_pass!($slot, after = []);
    };
    ($slot:ty, after = [$($after:ty),* $(,)?]) => {
        const _: () = {
            fn take(
                deck: &mut khora_core::lane::OutputDeck,
            ) -> Option<$crate::render::PassContribution> {
                deck.take::<$slot>().0
            }
            inventory::submit! {
                $crate::render::PassRegistration {
                    name: stringify!($slot),
                    type_id: std::any::TypeId::of::<$slot>,
                    after: &[$(std::any::TypeId::of::<$after>),*],
                    take,
                }
            }
        };
    };
}

// Shadows first: every lit pass samples the atlas they fill. The opaque scene
// gives the sky the depth it tests against; the blended scene composites over
// the sky; the UI and the debug overlays go on top.
register_pass!(ShadowPassSlot);
register_pass!(ScenePassSlot, after = [ShadowPassSlot]);
register_pass!(SkyboxPassSlot, after = [ScenePassSlot]);
register_pass!(TransparentPassSlot, after = [SkyboxPassSlot]);
register_pass!(UiPassSlot, after = [TransparentPassSlot]);
register_pass!(GridPassSlot, after = [UiPassSlot]);
register_pass!(WireframePassSlot, after = [GridPassSlot]);
register_pass!(GizmoPassSlot, after = [WireframePassSlot]);

/// Tag naming the encoder a lane records its transparent draws into.
///
/// A tag because [`LaneContext`](khora_core::lane::LaneContext) is keyed by
/// type, so the opaque encoder and this one cannot both be a bare
/// `dyn CommandEncoder` borrow: the agent lends it with
/// `insert_slot_as::<TransparentEncoder, _>` and a lane takes it with
/// `slot_as::<TransparentEncoder, dyn CommandEncoder>`. Only the lit lanes —
/// the ones with a blended batch — look for it; every other lane keeps
/// recording into the single encoder it always had.
pub struct TransparentEncoder;

/// Per-frame collection of recorded passes.
///
/// Agents append passes during `execute()`. `submit_frame_graph` drains the
/// graph in topological order and submits the buffers to the device.
#[derive(Default)]
pub struct FrameGraph {
    passes: Vec<RecordedPass>,
}

impl FrameGraph {
    /// Creates an empty frame graph.
    pub fn new() -> Self {
        Self { passes: Vec::new() }
    }

    /// Records a pass: descriptor + the command buffer the agent already produced.
    pub fn add_pass(&mut self, descriptor: PassDescriptor, command_buffer: CommandBufferId) {
        self.passes.push(RecordedPass {
            descriptor,
            command_buffer,
        });
    }

    /// Number of recorded passes.
    pub fn len(&self) -> usize {
        self.passes.len()
    }

    /// Whether the graph has no recorded passes.
    pub fn is_empty(&self) -> bool {
        self.passes.is_empty()
    }

    /// Drops all recorded passes without submitting them — used to recover
    /// from a frame error before the engine reaches the submit step.
    pub fn clear(&mut self) {
        self.passes.clear();
    }

    /// Drains the graph and returns command buffers in submission order.
    ///
    /// The order is computed by a stable Kahn's algorithm over read/write
    /// dependencies: a pass that writes resource `R` must run before any
    /// pass that reads `R`. Insertion order is the tie-breaker, so when the
    /// declared order is already valid (the common case) the result equals
    /// insertion order.
    pub fn compile(&mut self) -> Vec<CommandBufferId> {
        let passes = std::mem::take(&mut self.passes);
        sorted_passes(passes)
            .into_iter()
            .map(|p| p.command_buffer)
            .collect()
    }
}

/// Shared, mutable handle on the per-frame graph — registered as a service.
pub type SharedFrameGraph = Arc<Mutex<FrameGraph>>;

/// Drains `graph` and submits every recorded command buffer in topological
/// order. Returns the number of submitted buffers.
pub fn submit_frame_graph(graph: &SharedFrameGraph, device: &dyn GraphicsDevice) -> usize {
    let buffers = {
        let mut guard = graph.lock().expect("FrameGraph mutex poisoned");
        guard.compile()
    };
    let count = buffers.len();
    for buffer in buffers {
        device.submit_command_buffer(buffer);
    }
    count
}

// ─────────────────────────────────────────────────────────────────────────────
// Topological sort — stable Kahn's algorithm
// ─────────────────────────────────────────────────────────────────────────────

fn sorted_passes(passes: Vec<RecordedPass>) -> Vec<RecordedPass> {
    let n = passes.len();
    if n <= 1 {
        return passes;
    }

    // Edge i → j when pass i writes a resource that pass j reads (and j > i in
    // insertion order; the latest writer is the one that produces the value
    // for downstream readers).
    let mut last_writer: HashMap<ResourceId, usize> = HashMap::new();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut in_deg = vec![0usize; n];

    for (i, pass) in passes.iter().enumerate() {
        for read in &pass.descriptor.reads {
            if let Some(&writer) = last_writer.get(read) {
                if writer != i {
                    adj[writer].push(i);
                    in_deg[i] += 1;
                }
            }
        }
        for write in &pass.descriptor.writes {
            last_writer.insert(*write, i);
        }
    }

    // Of the passes whose inputs are ready, the earliest inserted goes first:
    // insertion order is the tie-breaker, so a pass freed late by its reads
    // still precedes a later-inserted pass freed early.
    let mut ready: BinaryHeap<Reverse<usize>> =
        (0..n).filter(|&i| in_deg[i] == 0).map(Reverse).collect();
    let mut order = Vec::with_capacity(n);
    let mut visited: HashSet<usize> = HashSet::new();
    while let Some(Reverse(i)) = ready.pop() {
        if !visited.insert(i) {
            continue;
        }
        order.push(i);
        for &j in &adj[i] {
            in_deg[j] -= 1;
            if in_deg[j] == 0 {
                ready.push(Reverse(j));
            }
        }
    }

    debug_assert_eq!(order.len(), n, "FrameGraph cycle detected");

    let mut indexed: Vec<Option<RecordedPass>> = passes.into_iter().map(Some).collect();
    order
        .into_iter()
        .map(|i| indexed[i].take().expect("pass already taken"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::renderer::api::command::CommandBufferId;

    fn buf(id: u64) -> CommandBufferId {
        CommandBufferId(id)
    }

    /// **The glass-against-the-sky bug.** A blended surface writes no depth,
    /// and the skybox paints exactly the pixels left at the far plane — so the
    /// sky must draw *before* the transparent geometry, or it repaints it
    /// wherever no opaque geometry stood behind.
    ///
    /// Insertion order is what expresses this: the topological sort refines it
    /// via declared read/write edges, and insertion order is the tie-breaker.
    /// This pins the order the engine folds the slots in.
    #[test]
    fn the_sky_is_folded_between_the_opaque_and_the_blended_draws() {
        let mut g = FrameGraph::new();

        // The order `EngineCore::submit_passes` uses.
        g.add_pass(
            PassDescriptor::new("ScenePass")
                .writes(ResourceId::Color)
                .writes(ResourceId::Depth),
            buf(1),
        );
        g.add_pass(
            PassDescriptor::new("SkyboxPass")
                .writes(ResourceId::Color)
                .reads(ResourceId::Depth),
            buf(2),
        );
        g.add_pass(
            PassDescriptor::new("TransparentPass")
                .writes(ResourceId::Color)
                .reads(ResourceId::Depth),
            buf(3),
        );

        assert_eq!(
            g.compile(),
            [buf(1), buf(2), buf(3)],
            "the sky must sit between the opaque draws that give it a depth \
             buffer and the blended draws that composite over it"
        );
    }

    /// **The shadow pass closes as the pass every lit pass waits for.** A
    /// `#[pass]` output opens on the slot's descriptor and closes into the
    /// slot: the shadow pass writes the shadow atlas, reads nothing, and holds
    /// the buffer its encoder finished into.
    #[test]
    fn the_shadow_pass_closes_writing_the_shadow_atlas() {
        use khora_core::lane::io::PassSlot;

        let closed = ShadowPassSlot::close(ShadowPassSlot::descriptor(), buf(7));
        let Some(pass) = closed.0 else {
            panic!("a closed shadow pass slot holds its pass");
        };
        assert_eq!(pass.descriptor.name, "ShadowPass");
        assert_eq!(pass.descriptor.writes, [ResourceId::ShadowAtlas]);
        assert!(
            pass.descriptor.reads.is_empty(),
            "the shadow pass reads no frame resource: {:?}",
            pass.descriptor.reads
        );
        assert_eq!(pass.command_buffer, buf(7));
    }

    /// **The skybox pass closes between the opaque and the blended draws.**
    /// It paints the frame's colour where the opaque pass left the depth at
    /// the far plane: it writes the colour, reads the depth, and holds the
    /// buffer its encoder finished into.
    #[test]
    fn the_skybox_pass_closes_writing_the_colour_and_reading_the_depth() {
        use khora_core::lane::io::PassSlot;

        let closed = SkyboxPassSlot::close(SkyboxPassSlot::descriptor(), buf(9));
        let Some(pass) = closed.0 else {
            panic!("a closed skybox pass slot holds its pass");
        };
        assert_eq!(pass.descriptor.name, "SkyboxPass");
        assert_eq!(pass.descriptor.writes, [ResourceId::Color]);
        assert_eq!(pass.descriptor.reads, [ResourceId::Depth]);
        assert_eq!(pass.command_buffer, buf(9));
    }

    /// **The grid pass closes writing the colour and reading the depth.** The
    /// ground grid is depth-tested against the scene, so closer geometry
    /// hides it: it reads the depth, paints the colour, and holds the buffer
    /// its encoder finished into.
    #[test]
    fn the_grid_pass_closes_writing_the_colour_and_reading_the_depth() {
        use khora_core::lane::io::PassSlot;

        let closed = GridPassSlot::close(GridPassSlot::descriptor(), buf(11));
        let Some(pass) = closed.0 else {
            panic!("a closed grid pass slot holds its pass");
        };
        assert_eq!(pass.descriptor.name, "GridPass");
        assert_eq!(pass.descriptor.writes, [ResourceId::Color]);
        assert_eq!(pass.descriptor.reads, [ResourceId::Depth]);
        assert_eq!(pass.command_buffer, buf(11));
    }

    /// **The wireframe pass closes writing the colour and reading the
    /// depth.** Hidden edges are occluded by the scene, so it reads the depth
    /// (and never writes it).
    #[test]
    fn the_wireframe_pass_closes_writing_the_colour_and_reading_the_depth() {
        use khora_core::lane::io::PassSlot;

        let closed = WireframePassSlot::close(WireframePassSlot::descriptor(), buf(12));
        let Some(pass) = closed.0 else {
            panic!("a closed wireframe pass slot holds its pass");
        };
        assert_eq!(pass.descriptor.name, "WireframePass");
        assert_eq!(pass.descriptor.writes, [ResourceId::Color]);
        assert_eq!(pass.descriptor.reads, [ResourceId::Depth]);
        assert_eq!(pass.command_buffer, buf(12));
    }

    /// **The gizmo pass closes writing the colour and reading nothing.**
    /// Gizmos are drawn on top of everything, with no depth test, so the pass
    /// declares no read of the depth.
    #[test]
    fn the_gizmo_pass_closes_writing_the_colour_only() {
        use khora_core::lane::io::PassSlot;

        let closed = GizmoPassSlot::close(GizmoPassSlot::descriptor(), buf(13));
        let Some(pass) = closed.0 else {
            panic!("a closed gizmo pass slot holds its pass");
        };
        assert_eq!(pass.descriptor.name, "GizmoPass");
        assert_eq!(pass.descriptor.writes, [ResourceId::Color]);
        assert!(
            pass.descriptor.reads.is_empty(),
            "the gizmos test no depth: {:?}",
            pass.descriptor.reads
        );
        assert_eq!(pass.command_buffer, buf(13));
    }

    /// **The overlays fold in their compositing order whatever they declare.**
    /// Added after the UI in the order the engine folds the slots — grid,
    /// wireframe, gizmos — with the descriptors their slots open with, the
    /// frame graph keeps that order: the grid and the wireframe read only the
    /// depth and the gizmos read nothing, so nothing but the insertion order
    /// puts the gizmos last.
    #[test]
    fn the_overlay_passes_keep_their_folding_order() {
        use khora_core::lane::io::PassSlot;

        let mut g = FrameGraph::new();
        g.add_pass(
            PassDescriptor::new("ScenePass")
                .writes(ResourceId::Color)
                .writes(ResourceId::Depth),
            buf(1),
        );
        g.add_pass(
            PassDescriptor::new("UiPass")
                .reads(ResourceId::Color)
                .writes(ResourceId::Color),
            buf(2),
        );
        g.add_pass(GridPassSlot::descriptor(), buf(3));
        g.add_pass(WireframePassSlot::descriptor(), buf(4));
        g.add_pass(GizmoPassSlot::descriptor(), buf(5));

        assert_eq!(g.compile(), [buf(1), buf(2), buf(3), buf(4), buf(5)]);
    }

    #[test]
    fn empty_graph_compiles_empty() {
        let mut g = FrameGraph::new();
        assert_eq!(g.compile().len(), 0);
    }

    #[test]
    fn single_pass_returned_as_is() {
        let mut g = FrameGraph::new();
        g.add_pass(PassDescriptor::new("scene"), buf(1));
        let order = g.compile();
        assert_eq!(order, vec![buf(1)]);
    }

    #[test]
    fn insertion_order_preserved_when_dependencies_match() {
        let mut g = FrameGraph::new();
        g.add_pass(
            PassDescriptor::new("shadow").writes(ResourceId::ShadowAtlas),
            buf(1),
        );
        g.add_pass(
            PassDescriptor::new("scene")
                .reads(ResourceId::ShadowAtlas)
                .writes(ResourceId::Color),
            buf(2),
        );
        g.add_pass(
            PassDescriptor::new("ui")
                .reads(ResourceId::Color)
                .writes(ResourceId::Color),
            buf(3),
        );
        assert_eq!(g.compile(), vec![buf(1), buf(2), buf(3)]);
    }

    #[test]
    fn reads_force_writer_first_even_if_inserted_late() {
        // A reader inserted before the writer is allowed: insertion-order tie
        // breaks ties only when no edge is established. Here we test that
        // edges are correctly built when reads precede writes in the input.
        let mut g = FrameGraph::new();
        // pass0 reads X but no one writes X yet → no edge.
        g.add_pass(PassDescriptor::new("a").reads(ResourceId::Color), buf(1));
        // pass1 writes X.
        g.add_pass(PassDescriptor::new("b").writes(ResourceId::Color), buf(2));
        // pass2 reads X → must be after pass1.
        g.add_pass(PassDescriptor::new("c").reads(ResourceId::Color), buf(3));

        let order = g.compile();
        let pos_b = order.iter().position(|b| *b == buf(2)).unwrap();
        let pos_c = order.iter().position(|b| *b == buf(3)).unwrap();
        assert!(pos_b < pos_c);
    }

    struct FirstLoopSlot;
    struct SecondLoopSlot;

    fn take_nothing(_deck: &mut khora_core::lane::OutputDeck) -> Option<PassContribution> {
        None
    }

    /// **A pass order with a cycle is refused, naming both slots.** Two slots
    /// that each declare they come after the other have no order the frame
    /// could be submitted in; the engine refuses them at boot rather than
    /// guessing one, and the refusal says which two they are.
    #[test]
    fn a_pass_order_cycle_is_refused_at_boot() {
        static FIRST: PassRegistration = PassRegistration {
            name: "FirstLoopSlot",
            type_id: std::any::TypeId::of::<FirstLoopSlot>,
            after: &[std::any::TypeId::of::<SecondLoopSlot>],
            take: take_nothing,
        };
        static SECOND: PassRegistration = PassRegistration {
            name: "SecondLoopSlot",
            type_id: std::any::TypeId::of::<SecondLoopSlot>,
            after: &[std::any::TypeId::of::<FirstLoopSlot>],
            take: take_nothing,
        };

        let refusal = match pass_order(&[&FIRST, &SECOND]) {
            Ok(order) => panic!(
                "a cycle was ordered: {:?}",
                order.iter().map(|pass| pass.name).collect::<Vec<_>>()
            ),
            Err(refusal) => refusal,
        };
        let message = refusal.to_string();
        for name in ["FirstLoopSlot", "SecondLoopSlot"] {
            assert!(
                refusal.cycle.contains(&name),
                "the cycle names {name}: {:?}",
                refusal.cycle
            );
            assert!(
                message.contains(name),
                "the message names {name}: {message}"
            );
        }
    }

    #[test]
    fn clear_drops_passes() {
        let mut g = FrameGraph::new();
        g.add_pass(PassDescriptor::new("scene"), buf(1));
        g.add_pass(PassDescriptor::new("ui"), buf(2));
        assert_eq!(g.len(), 2);
        g.clear();
        assert!(g.is_empty());
    }
}
