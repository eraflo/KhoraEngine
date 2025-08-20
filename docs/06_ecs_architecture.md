# 06 - ECS Architecture: The Chunked Relational Page ECS (CRPECS)

This document details the architecture of Khora's custom Entity-Component-System, designed from the ground up to be the backbone of the Symbiotic Adaptive Architecture (SAA) and its most advanced concept, Adaptive Game Data Flows (AGDF).

## Philosophy: Performance Through Intelligent Compromise

Modern ECS architectures force a difficult choice:
*   **Archetypal (e.g., `bevy_ecs`)**: Extremely fast for iteration, but very slow when an entity's structure (its set of components) changes.
*   **Sparse Set (e.g., `specs`)**: Very fast for structural changes, but slower for iterating over entities with many components due to poor data locality.

Khora's vision requires both: fast, predictable iteration for the **Data Plane**, and cheap, frequent structural changes driven by the **Control Plane** (SAA). The CRPECS is designed to resolve this conflict by creating a new set of trade-offs perfectly aligned with our goals.

Its core principle is to **dissociate an entity's identity from the physical location of its data**.

## Core Principles

### 1. Component Pages: Semantic Data Grouping

Instead of storing components by entity (Archetype) or by type (Sparse Set), we group them by **semantic domain** into fixed-size memory blocks called **Pages** (e.g., 16KB).

*   A **`PhysicsPage`** stores `Vec<Position>`, `Vec<Velocity>`, `Vec<Collider>`.
*   A **`RenderPage`** stores `Vec<MeshHandle>`, `Vec<MaterialHandle>`, `Vec<Visibility>`.

Within a page, data is stored as a **Structure of Arrays (SoA)**, guaranteeing optimal cache performance for iteration within that domain.

### 2. Entity Metadata: The Relational Hub

An entity's identity is maintained in a central table, separate from its component data. Each entry in this table is a small struct containing pointers to the entity's data across all relevant pages.

```rust
// A logical pointer to a piece of component data.
struct PageIndex {
    page_id: u32,   // Which page holds the data?
    row_index: u32, // Which row in that page?
}

// The central "table of contents" for a single entity.
struct EntityMetadata {
    physics_location: Option<PageIndex>,
    render_location: Option<PageIndex>,
    ai_location: Option<PageIndex>,
    // ... one location per semantic domain
}
```

### 3. Data Dissociation & Flexibility

The data for a single entity is physically scattered across multiple Pages, but it is logically unified by its `EntityMetadata`. This is the key to our flexibility.

## How Key Operations Work

#### Iteration (e.g., `Query<(&mut Position, &Velocity)>`)
*   **Performance: Optimal.**
*   The query knows that `Position` and `Velocity` live in `PhysicsPage`s. It simply iterates through each `PhysicsPage` and its contiguous `Vec`s, achieving performance nearly identical to a pure Archetypal ECS.

#### Adding/Removing Components (Structural Changes)
*   **Performance: Very High.**
*   To remove an entity's AI behavior, we simply set `ai_location = None` in its `EntityMetadata`. This is a near-instantaneous `O(1)` operation. The actual data becomes "orphaned" and is cleaned up later by a garbage collection process.
*   To add components, we only need to move the data for that specific semantic group (e.g., moving physics data to a new `PhysicsPage` that can accommodate a `Collider`). The entity's render and AI data are completely unaffected and do not move.

## The Intelligent Compromise: Transversal Queries

The trade-off for this flexibility is the cost of "transversal" queries—those that access data from different semantic domains simultaneously (e.g., `Query<(&Position, &MeshHandle)>`).

*   **Mechanism**: Such a query cannot iterate linearly. It must perform a "join," typically by iterating over one set of pages and using the `EntityMetadata` table to look up the corresponding data in the other set of pages.
*   **Cost**: This lookup process breaks data locality and introduces cache misses. A transversal query can be **2x to 10x slower** than a native query.
*   **Architectural Benefit**: This design strongly encourages clean, decoupled systems. The physics engine should rarely need to know about rendering data. We make the common case (domain-specific queries) extremely fast, at the expense of the rare case.

## Integration with CLAD and SAA

The CRPECS is the cornerstone of the **`khora-data`** crate. It is the ultimate implementation of the **[D]ata** in CLAD.
*   It provides the perfect foundation for **Adaptive Game Data Flows (AGDF)**, as the SAA's Control Plane can cheaply and frequently alter data layouts by modifying `EntityMetadata`.
*   The **garbage collection** and page compaction processes can themselves be implemented as **Intelligent Subsystem Agents (ISAs)**, operating when the system has spare resources, making the ECS a living, self-optimizing part of the SAA.