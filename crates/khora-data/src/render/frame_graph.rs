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

use std::collections::{HashMap, HashSet, VecDeque};
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
        Self {
            passes: Vec::new(),
        }
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

    let mut queue: VecDeque<usize> = (0..n).filter(|&i| in_deg[i] == 0).collect();
    let mut order = Vec::with_capacity(n);
    let mut visited: HashSet<usize> = HashSet::new();
    while let Some(i) = queue.pop_front() {
        if !visited.insert(i) {
            continue;
        }
        order.push(i);
        for &j in &adj[i] {
            in_deg[j] -= 1;
            if in_deg[j] == 0 {
                queue.push_back(j);
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
