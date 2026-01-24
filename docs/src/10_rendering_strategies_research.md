# 10. Research: Rendering Strategies & Adaptivity

This document captures the initial research and architectural vision for the **Rendering ISA** and its multiple strategies..

## The Vision: Polymorphic Rendering

In a conventional game engine, the rendering pipeline (Forward, Deferred, etc.) is a project-wide choice. Khora's **Symbiotic Adaptive Architecture (SAA)** treats rendering as a **polymorphic service**.

The goal is not just to support multiple rendering techniques, but to enable the engine to **seamlessly switch** between them at runtime based on environmental context and performance goals.

### 1. Coexistence vs. Selection

Instead of a static selection, Khora maintains several **RenderLanes**, each implementing a specific `RenderLane` strategy.

*   **ForwardPlusLane (Implemented)**: Excellent for high-quality MSAA, transparency, and scenes with moderate light counts.
*   **SimpleUnlitLane (Implemented)**: Minimalistic path for UI, debug views, or extremely low-end hardware.
*   **DeferredLane (Concept)**: Ideal for scenes with massive light counts where lighting complexity needs to be decoupled from geometric complexity.
*   **Mobile/Low-Power Lane (Concept)**: A strategy focused on energy efficiency and thermal management.
*   **Virtual Geometry/Texture Lane (Concept)**: A specialized path (Nanite-like) where the Lane itself manages fine-grained asset streaming and visibility to handle extreme complexity.

### 2. The Strategy Contract

For the **DCC (Dynamic Context Core)** to make intelligent decisions, every `RenderLane` must adhere to a strict contract defined in `khora-core`. A strategy is not just a shader; it's a resource-aware worker that must report:

*   **Static Resource Footprint**: How much VRAM is required for permanent buffers (G-Buffers, LUTs)?
*   **Complexity Scaling**: How does the GPU cost scale with vertex count, draw calls, and light counts?
*   **Capabilities**: Does it support transparency? MSAA? Real-time shadows?

### 3. GORNA & Strategy Switching

The **GORNA (Goal-Oriented Resource Negotiation & Allocation)** protocol uses these reports to perform live trade-offs.

> [!NOTE]
> **Example Scenario**:
> If the engine detects a sudden spike in dynamic lights while the frame budget is being exceeded, the `RenderAgent` might negotiate a downgrade from a high-quality `ForwardPlusLane` to a more efficient `DeferredLane` or even a simplified lighting model to maintain the target FPS.

## Integration with CLAD

The rendering strategies are the perfect embodiment of the **CLAD Pattern**:

*   **[C]ontrol**: The `RenderAgent` manages the lifecycle of lanes and listens to the DCC.
*   **[L]ane**: The `RenderLane` implementations (`ForwardPlusLane`, etc.) contain the optimized, deterministic GPU command recording logic.
*   **[A]gent**: The `RenderAgent` acts as an ISA, negotiating for GPU time and memory budget.
*   **[D]ata**: The `RenderWorld` acts as the decoupled data structure, allowing any lane to consume the extracted scene state.

## Future Research Areas

*   **NPR (Non-Photorealistic Rendering) Lanes**: Styles like Cell-shading or Oil-painting as selectable strategies.
*   **Hardware-Specific Lanes**: Specialized paths for hardware supporting Ray Tracing or Mesh Shaders.
*   **Autonomous Streaming Lanes**: Researching how a Lane can bypass traditional asset loading to manage its own "just-in-time" geometry or texture streaming (Nanite/Virtual Textures).
*   **Decision Heuristics**: Training the DCC to recognize which strategy is optimal for various scene compositions.
