# 2. Core Concepts: The Symbiotic Adaptive Architecture (SAA)

The Symbiotic Adaptive Architecture (SAA) is the philosophical and conceptual framework of Khora. It is built on seven key pillars that work in symbiosis to create a truly adaptive engine.

### 1. Dynamic Context Core (DCC) - The Central Nervous System

The DCC is the engine's center of awareness. It does not command subsystems directly; instead, it maintains a constantly updated **situational model** of the entire application state. It acts as a central hub, aggregating telemetry from across the engine to understand the "big picture":
*   **Hardware Load**: Real-time utilization of CPU cores, GPU, VRAM, and memory bandwidth.
*   **Game State**: Scene complexity, entity counts, light sources, physics interactions, network status.
*   **Performance Goals**: The currently active objectives, such as target framerate, maximum input latency, or power consumption budget.

### 2. Intelligent Subsystem Agents (ISAs) - The Specialists

Every major engine subsystem (Rendering, Physics, Audio, AI, Assets) is designed as an Intelligent Subsystem Agent. An ISA is not a passive library; it is a semi-autonomous component with a deep understanding of its own domain.
*   **Self-Assessment**: It constantly measures its own performance and resource consumption.
*   **Multi-Strategy**: It possesses multiple algorithms to accomplish its task, each with different performance characteristics (e.g., a precise but slow physics solver vs. a fast but approximate one).
*   **Cost Estimation**: It can accurately predict the resource cost (CPU time, memory) of each of its strategies under the current conditions.

### 3. Goal-Oriented Resource Negotiation & Allocation (GORNA) - The Council Protocol

GORNA is the formal communication protocol used by the DCC and the ISAs to dynamically allocate resources. This negotiation process replaces static, pre-defined budgets.
1.  **Request**: ISAs submit their desired resource needs to the DCC, often specifying the strategy they intend to use (e.g., "The Rendering Agent requests 8ms of GPU time to execute its High-Fidelity strategy").
2.  **Arbitration**: The DCC analyzes all incoming requests, comparing them against its global situational model and the active performance goals.
3.  **Allocation**: The DCC grants a final **budget** to each ISA. This budget may be less than what was requested.
4.  **Adaptation**: An ISA that receives a reduced budget is responsible for adapting. It must select a less resource-intensive strategy to stay within its allocated budget.

> **Implementation Status**: GORNA v0.3 is fully operational. The DCC runs 9 heuristics each tick (Phase, Thermal, Battery, Frame Time, Stutter, Trend, CPU/GPU Pressure, Death Spiral), the `GornaArbitrator` resolves multi-agent resource conflicts, and the `RenderAgent` implements cost-based negotiation with VRAM-aware filtering. The `PhysicsAgent` is the second fully GORNA-compliant ISA. An initial GORNA round fires automatically on the first tick after agent registration, ensuring baseline budgets are assigned immediately. All lanes across the engine implement the unified `Lane` trait with `LaneContext`-based dispatch (see [Chapter 4](04_technical_architecture.md) and [Chapter 10](10_rendering_strategies_research.md)). See [Chapter 11](11_dcc_architecture.md) and [Chapter 12](12_gorna_protocol.md) for the complete specification.

### 4. Adaptive Game Data Flows (AGDF) - The Living Data

AGDF is the principle that not only algorithms but also the very structure of data should be dynamic. This advanced concept is realized through our custom ECS, the **CRPECS**. Instead of being static, an entity's data layout can be fundamentally altered by the SAA in response to the game's context. For example, the Control Plane can cheaply remove physics components from an entity that is far from the player, and add them back when it gets closer. The CRPECS's design makes these structural changes extremely low-cost, enabling a deeper level of self-optimization at the memory level.

### 5. Semantic Interfaces & Contracts - The Common Language

For intelligent negotiation to be possible, all ISAs must speak a common, unambiguous language. These are defined by a set of formal contracts (Rust traits) that specify an ISA's capabilities and requirements.
*   **Capabilities**: What an ISA can do ("I can render scenes using Forward+ or a Simple Unlit pipeline").
*   **Requirements**: What data it needs to function ("I require access to all entity positions and meshes").
*   **Guarantees**: What it promises in return for a given budget ("With a 4ms CPU budget, I guarantee a stable physics simulation for up to 1000 active rigid bodies").

### 6. Observability & Traceability - The Glass Box

An intelligent system risks becoming an indecipherable "black box." To prevent this, Observability is a first-class principle in Khora. Every significant decision made by the DCC is meticulously logged with its complete context—the telemetry, the requests, and the final budget allocation. This allows developers to ask not just "what happened?" but "**why** did the engine make that choice?", which is crucial for debugging, tuning, and building trust in the adaptive system.

### 7. Developer Guidance & Control - A Partnership, Not an Autocracy

The engine's autonomy serves the developer, it does not replace them. Khora provides clear mechanisms for developers to guide and constrain the SAA.
*   **Constraints**: Developers can define rules or physical volumes in the world to influence decision-making (e.g., "In this zone, physics accuracy is more important than graphical fidelity").
*   **Adaptation Modes** *(planned)*: The DCC will be able to operate in different modes:
    *   `Learning`: The default, fully dynamic mode where the engine explores strategies to meet its goals.
    *   `Stable`: Uses heuristics learned from the `Learning` mode but avoids drastic changes. Ideal for profiling and shipping a predictable experience.
    *   `Manual`: Disables the SAA entirely, allowing developers to lock in specific strategies for debugging or benchmarking.