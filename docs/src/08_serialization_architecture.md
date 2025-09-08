# 08. Serialization Architecture: SAA-Serialize

### Core Principle: Intent-Driven Persistence

In Khora Engine, scene serialization is not a simple, monolithic operation. It is a direct application of the **Symbiotic Adaptive Architecture (SAA)** philosophy, designed to be flexible, performant, and future-proof. We call this system **SAA-Serialize**.

The core principle is that there is no single "best" way to save and load a scene. The optimal method depends entirely on the developer's **goal**. Saving a level for a shipping build requires maximum loading speed. Saving data for debugging requires a human-readable format. Saving a game state for a long-term project needs a format that will not be invalidated by future engine updates.

SAA-Serialize solves this by separating the **intent** of serialization from its **implementation**. The developer expresses their goal, and an intelligent agent selects the best strategy to achieve it.

### A Note on RON (Rusty Object Notation)

Throughout this document, we refer to RON as the format of choice for human-readable output. RON is a data serialization format designed specifically for the Rust ecosystem. Think of it as a superset of Rust's struct syntax, making it instantly familiar to any Rust developer.

Key features of RON include:
*   **Human-Readability:** Its syntax is clean, allows for comments, and is generally less verbose than formats like XML.
*   **Expressiveness:** It natively supports all of Rust's data types, including enums and tuples, which are often awkward to represent in formats like JSON.
*   **Ecosystem:** It is a well-supported `serde` format, making its integration into Khora seamless.

Whenever a `SerializationGoal` prioritizes debuggability or readability, RON will be the underlying text format used by the chosen strategy.

### Advantages of the SAA-Serialize Approach

*   **Maximum Flexibility:** The engine is never locked into a single format. It can use the best tool for every job, seamlessly switching between binary, text, stable, or raw-memory formats.
*   **Optimized Performance:** For production builds, the system can use highly optimized strategies to achieve the fastest possible loading times, without compromising the stability of developer tools.
*   **Guaranteed Stability:** For archival and team collaboration, the system can use a stable, versioned format completely decoupled from the engine's internal memory layout, ensuring files remain valid across engine versions.
*   **Future-Proof Extensibility:** The architecture is designed to be extended. New serialization techniques can be added to the engine as new `Lanes` without ever breaking the public API.

### Built-in Serialization Strategies (Lanes)

Khora Engine ships with a core set of distinct serialization strategies. The `SerializationAgent` chooses from these `Lanes` based on the `SerializationGoal` specified by the developer.

#### 1. The Definition Strategy (The Stable Lane)

*   **Primary Goal:** `SerializationGoal::LongTermStability`, `SerializationGoal::HumanReadableDebug` (using the RON format).
*   **Principle:** This strategy converts the live `World` into a simple, stable, intermediate representation—a `SceneDefinition`—before writing it to disk. This format describes the scene in logical terms ("this entity has a Transform component and a Material component") rather than in terms of memory layout.
*   **Key Characteristics:**
    *   **Pro:** Extremely robust. Changes to component memory layout or ECS internals will not invalidate scene files. This is the safest choice for long-term asset stability.
    *   **Pro:** Can be written in a human-readable format like RON, making it perfect for debugging and version control.
    *   **Con:** This is the slowest strategy, as it requires an intermediate memory allocation for the `SceneDefinition` and a full data conversion.

#### 2. The Recipe Strategy (The Dynamic Lane)

*   **Primary Goal:** `SerializationGoal::HumanReadableDebug`, and serves as a foundation for editor tools.
*   **Principle:** This strategy represents the scene not as a snapshot of data, but as a sequential list of commands—a "recipe"—that can be executed to reconstruct the world. The file contains instructions like `SpawnEntity(ID)`, `AddComponent(EntityID, ComponentData)`, etc.
*   **Key Characteristics:**
    *   **Pro:** A highly flexible format, ideal for implementing editor features like undo/redo, scene patching (prefabs), and potentially content streaming.
    *   **Pro:** Can also be serialized to a text format, making the scene's construction process transparent and easy to debug.
    *   **Con:** Loading can be less performant than bulk data loading due to the overhead of executing a long series of individual commands.

#### 3. The Archetype Strategy (The Performance Lane)

*   **Primary Goal:** `SerializationGoal::FastestLoad`.
*   **Principle:** A pure, data-oriented performance strategy. It bypasses all logical abstractions and serializes the CRPECS memory as directly as possible. It takes a snapshot of each archetype's component arrays and writes them to disk as contiguous blocks of data.
*   **Key Characteristics:**
    *   **Pro:** By far the fastest possible loading method. Deserialization is nearly equivalent to a direct memory copy (`memcpy`), minimizing CPU work.
    *   **Con:** Extremely fragile. The file is tightly coupled to the exact component memory layout, engine version, compiler, and target architecture. The slightest change will invalidate it. This is **only** suitable for final, "cooked" game builds for a specific platform.
    *   **Con:** The format is an opaque binary blob, impossible to read or debug.

#### 4. The Delta Strategy (The Efficiency Lane)

*   **Primary Goal:** `SerializationGoal::SmallestFileSize`.
*   **Principle:** This strategy only saves the differences ("deltas") between the current `World` state and a reference `World` state (e.g., the base level scene).
*   **Key Characteristics:**
    *   **Pro:** Ideal for game saves or content patches where only a few elements have changed, resulting in very small files. This is also the foundational concept for network state replication.
    *   **Con:** The algorithm to compute the difference between two scenes is complex and can be slow. Loading requires first loading the base scene and then applying the delta patch.

### Developer Extensibility: Creating Custom Lanes

The SAA-Serialize system is designed to be open. The engine will export the public `SerializationStrategy` trait, allowing developers to implement and register their own custom `Lanes` with the `SerializationAgent`.

This allows a studio to:
*   Integrate with proprietary, in-house file formats.
*   Create `Lanes` that export to standard formats like JSON or XML for interoperability with external tools.
*   Develop highly specialized binary formats tailored to their game's unique needs or specific hardware.

This extensibility ensures that developers are never limited by the built-in strategies and can adapt Khora's persistence system to fit any production pipeline.