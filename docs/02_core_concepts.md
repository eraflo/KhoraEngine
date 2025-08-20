# 02 - Core Concepts: The Symbiotic Adaptive Architecture (SAA)

The SAA is Khora's conceptual framework. It is built on several key pillars that work in symbiosis.

### 1. Dynamic Context Core (DCC) - The Brain

The DCC is the engine's center of awareness. It doesn't command directly, but maintains a constantly updated **situational model**. It aggregates information from across the engine:
*   **Hardware Load**: CPU, GPU, VRAM, and memory bandwidth utilization.
*   **Game State**: Entity counts, scene complexity, light counts, network status.
*   **Performance Goals**: Target framerate, maximum latency, power budget.

### 2. Intelligent Subsystem Agents (ISAs) - The Specialists

Each major subsystem (Rendering, Physics, Audio, AI, Networking, Assets) is encapsulated as an Intelligent Agent. An ISA is not just a set of functions; it is semi-autonomous and has specific capabilities:
*   **Self-Assessment**: It can measure its own performance and resource usage.
*   **Multi-Strategy**: It knows several ways to do its job (e.g., a precise but slow physics solver, or a fast but approximate one).
*   **Cost Estimation**: It can estimate the cost (in CPU, memory, etc.) of each of its strategies.

### 3. Goal-Oriented Resource Negotiation & Allocation (GORNA) - The Negotiation

GORNA is the communication protocol between the DCC and the ISAs. Instead of static resource allocation, a negotiation takes place:
1.  **Request**: ISAs submit their needs to the DCC ("I need 5ms of CPU for my high-fidelity strategy").
2.  **Arbitration**: The DCC analyzes all requests, comparing them against its situational model and global goals.
3.  **Allocation**: The DCC allocates **budgets** to each ISA.
4.  **Adaptation**: An ISA that receives a budget lower than its request must select a cheaper strategy to stay within it.

### 4. Adaptive Game Data Flows (AGDF) - The Living Data

AGDF is the most advanced and experimental concept, and it is being realized through our custom ECS architecture, the **Chunked Relational Page ECS (CRPECS)**. The idea is that even the structure of data should not be static.

Instead of just changing algorithms, the SAA can fundamentally alter how an entity's data is laid out in memory. For instance, the Control Plane can cheaply and frequently change an entity's component structure to match its current context (e.g., removing complex physics components when it is far from the player). This is made possible by the CRPECS's design, which decouples an entity's identity from the physical storage of its data, making such structural changes extremely low-cost. This is a core part of Khora's self-optimization strategy at the deepest level.

### 5. Semantic Interfaces & Contracts - The Common Language

For negotiation to work, ISAs must communicate clearly. Semantic contracts are interfaces that define:
*   **Capabilities**: What an ISA can do ("I can render in deferred mode with up to 32 lights").
*   **Requirements**: What it needs to function ("I need the position and velocity of all rigid bodies").
*   **Guarantees**: What it promises in return for a budget ("With 4ms of CPU, I guarantee a stable physics simulation for 1000 objects").

### 6. Observability & Traceability - Avoiding the Black Box

A system this intelligent risks becoming unpredictable. Observability is a first-class principle, not an afterthought. Every decision made by the DCC is logged with its complete context, allowing developers to ask not just "what went wrong?" but "**why** did the engine make that choice?". This is crucial for debugging and building trust in the system.

### 7. Developer Guidance & Control - Partnership, Not Autocracy

The engine's autonomy must not override developer intent. Khora provides mechanisms for developers to **guide** the SAA:
*   **Constraints**: Developers can define rules or volumes where certain subsystems are prioritized (e.g., "physics is critical in this area").
*   **Adaptation Modes**: The DCC can be switched between modes:
    *   `Learning`: The default, fully dynamic mode.
    *   `Stable`: Uses learned heuristics but avoids drastic changes, ideal for profiling and shipping.
    *   `Manual`: Disables the SAA, allowing developers to force specific strategies for debugging.