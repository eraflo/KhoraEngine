    
# 03 - Technical Architecture: The CLAD Pattern

The **CLAD (Control-Lane-Agent-Data)** pattern is the concrete Rust implementation of the SAA philosophy. It is designed for maximum performance by strictly separating slow, complex decision-making from fast, deterministic execution.

### Core Principle: Hot/Cold Path Separation

The entire architecture is built on isolating the **Control Plane (Cold Path)**, where thinking happens, from the **Data Plane (Hot Path)**, where the raw work is done every frame.

*   **Cold Path (Control Plane)**:
    *   **Tasks**: DCC analysis, GORNA negotiation, strategy selection.
    *   **Frequency**: Ticks at a lower rate (e.g., 30-120 Hz).
    *   **Characteristics**: Can allocate memory, use complex logic, write logs. Does not directly impact the render time of a frame.

*   **Hot Path (Data Plane)**:
    *   **Tasks**: Execution of rendering, physics, and other pipelines.
    *   **Frequency**: Must execute within a single frame's budget (e.g., < 16.67ms for 60fps).
    *   **Characteristics**: Zero heap allocations, cache-friendly code, SIMD operations, maximum predictability.

### The CLAD Components

#### C - `khora-control` (Control Plane)
The strategic brain. It contains the **DCC** and the **GORNA** solver. It makes decisions based on reports from Agents but never performs low-level work like rendering. It only depends on `khora-core`.

#### L - `khora-lanes` (Data Plane)
The ultra-fast, deterministic execution pipelines. A "Lane" is an optimized pipeline that does one thing, but does it very quickly (e.g., `render-lane`, `physics-lane`). They are "dumb" by design: they make no decisions, they only execute the configuration provided by an Agent.

#### A - `khora-agents` (The Bridge)
Agents are the intelligent wrappers that connect the Control Plane to the Data Plane. An Agent:
1.  Implements the `ISA` interface to communicate with the `DCC`.
2.  Knows multiple strategies.
3.  Receives a command from the `DCC` (e.g., "switch to 'high-performance' mode").
4.  Translates this command into a concrete configuration for its associated `Lane`.

#### D - `khora-data` (The Foundation)
The subsystem that manages data. It provides the tools for AGDF (AoS<>SoA transformations), specialized allocators (frame arenas to avoid hot-path allocations), and manages data placement (RAM/VRAM).

### State Synchronization & The Feedback Loop
A critical challenge is the time delay between the hot and cold paths. The Control Plane makes decisions based on data from previous frames. To mitigate this:
1.  **Context Snapshotting**: Metrics from the hot path are precisely timestamped and snapshotted at a defined point in the frame (e.g., end of frame).
2.  **Predictive Heuristics**: The DCC can use data from the last N frames to predict the state of the *next* frame, allowing it to make proactive rather than purely reactive decisions.
3.  **Command Queuing**: Decisions from the Control Plane are queued and applied to the Data Plane at a safe synchronization point, preventing race conditions.

### Simplified Dependency Graph

  

khora-bin (Composition Root)
|
v
+-----------------+ +----------------+
| khora-agents |----->| khora-lanes |
+-----------------+ +----------------+
|                           |
v                           v
+-----------------+ +----------------+
| khora-control |----->| khora-data |
+-----------------+ +----------------+
| |
| v
+----------------------->+
v
+--------------+
| khora-core | (Traits & Types)
+--------------+
^
|
+--------------+
| khora-infra | (Implementations: WGPU, etc.)
+--------------+

    
This structure ensures that decision logic (`control`) can never depend on implementation details (`infra`) and that the fast pipelines (`lanes`) remain pure and free of business logic.

  