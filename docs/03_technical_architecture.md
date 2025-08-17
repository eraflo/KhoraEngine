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

### The CLAD Crates

#### `khora-core` - The Foundation
Contains only abstract traits, universal data types, and pure utilities. It has no knowledge of any specific implementation. It defines the "language" of the engine.

#### `khora-data` - The Data Layer
Contains concrete implementations for data management: specialized allocators, data layout transformers (for AGDF), and streaming logic.

#### `khora-lanes` - The Hot Path
Contains the performance-critical, "dumb" execution pipelines (rendering passes, physics solvers). Optimized for speed, with no branching logic.

#### `khora-agents` - The Tactical Brains
Each agent is a smart wrapper around one or more `Lanes`. It knows about different strategies (e.g., Forward vs. Deferred rendering), estimates their costs, and reports to the `Control Plane`. It translates high-level commands into concrete `Lane` configurations.

#### `khora-control` - The Strategic Brain
The highest level of decision-making. Contains the **DCC** and **GORNA**. It consumes telemetry, evaluates the overall situation against high-level goals, and orchestrates the `Agents`.

#### `khora-telemetry` - The Nervous System
A dedicated service for collecting, storing, and exposing engine-wide metrics and monitoring data. It gathers data from `khora-infra` and provides it to `khora-control` and debugging tools.

#### `khora-infra` - The Bridge to the World
Contains all concrete implementations that interact with the outside world: GPU backends (WGPU), windowing (Winit), filesystem I/O, etc. It implements the traits defined in `khora-core`.

#### `khora-sdk` - The Public Facade
A simple, stable API for game developers. It hides the complexity of the internal CLAD architecture and provides easy-to-use entry points like `Engine::new()`.