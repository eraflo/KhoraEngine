# 6. ECS Architecture: The CRPECS

This document details the architecture of Khora's custom **Chunked Relational Page ECS (CRPECS)**. It was designed from the ground up to be the high-performance backbone of the SAA and to enable its most advanced concept: **Adaptive Game Data Flows (AGDF)**.

## Philosophy: Performance Through Intelligent Compromise

Modern ECS architectures typically force a difficult choice:
*   **Archetypal (e.g., `bevy_ecs`)**: Delivers extremely fast iteration speeds due to perfect data locality, but incurs a very high performance cost when an entity's component structure changes (an "archetype move").
*   **Sparse Set (e.g., `specs`)**: Offers very fast structural changes, but iteration can be slower for entities with many components due to increased pointer indirection and poorer data locality.

Khora's SAA vision requires the best of both worlds: fast, predictable iteration for the **Data Plane** (the `Lanes`), and cheap, frequent structural changes driven by the **Control Plane** (the `Agents` and `Control` crates). The CRPECS resolves this conflict by creating a new set of trade-offs perfectly aligned with our goals.

Its core principle is to **completely dissociate an entity's logical identity from the physical storage location of its component data**.

## Core Principles

### 1. Component Pages: Semantic Data Grouping

Instead of storing components based on the entity's complete structure (its archetype), we group them by **semantic domain** into fixed-size memory blocks called **Pages** (e.g., 16KB).
*   A **`PhysicsPage`** would store the contiguous `Vec`s for `Position`, `Velocity`, and `Collider`.
*   A **`RenderPage`** would store the contiguous `Vec`s for `MeshHandle`, `MaterialHandle`, and `Visibility`.

Within each Page, component data is stored as a **Structure of Arrays (SoA)**, guaranteeing optimal cache performance for any system iterating *within that domain*.

### 2. Entity Metadata: The Relational Hub

An entity's identity is maintained in a central table, completely separate from its component data. Each entry in this table is a small struct containing pointers that map the entity's ID to the physical location of its data across all relevant Pages.

```rust
// A logical pointer to a piece of component data.
struct PageIndex {
    page_id: u32,   // Which page holds the data?
    row_index: u32, // Which row within that page's SoA?
}

// The central "table of contents" for a single entity.
struct EntityMetadata {
    // A map from a component's semantic domain to its physical location.
    locations: HashMap<SemanticDomain, PageIndex>,
}
```

### 3. Data Dissociation: The Key to Flexibility

The data for a single entity is physically scattered across multiple Pages, but it is logically unified by its `EntityMetadata`. This is the architectural key that unlocks our required flexibility.

## How Key Operations Work

#### Iteration (e.g., `Query<(&mut Position, &Velocity)>`)
*   **Performance: Near-Optimal.**
*   The query understands that `Position` and `Velocity` both belong to the `Physics` semantic domain. It therefore only needs to iterate through all active `PhysicsPage`s, accessing their contiguous `Vec`s. This achieves performance nearly identical to a pure Archetypal ECS for domain-specific queries.

#### Adding/Removing Components (Structural Changes)
*   **Performance: Extremely High.**
*   To remove an entity's AI behavior, we simply remove an entry from the `locations` `HashMap` in its `EntityMetadata`. This is a near-instantaneous `O(1)` operation. The actual component data is now "orphaned" and will be cleaned up later by a low-priority garbage collection process.
*   Adding a component only requires modifying the data within a single semantic domain, leaving all other component data for that entity completely untouched.

## The Intelligent Compromise: Transversal Queries

The explicit trade-off for this flexibility is the performance cost of **transversal queries**—queries that access data from different semantic domains simultaneously (e.g., `Query<(&Position, &MeshHandle)>`).

*   **Mechanism**: Such a query cannot iterate linearly over a single set of Pages. It must perform a "join," typically by iterating over the pages of one domain (e.g., `PhysicsPage`s) and using each entity's ID to look up its `EntityMetadata`, which then points to the location of its data in the other domain (e.g., `RenderPage`s).
*   **Cost**: This lookup process introduces pointer chasing and potential cache misses. A transversal query can be significantly slower than a native, domain-specific query.
*   **Architectural Benefit**: This is a deliberate design choice. It creates a strong "architectural gravity" that encourages developers to write clean, decoupled systems. The physics engine should operate on physics data, and the rendering engine on rendering data. We make the common, well-designed case extremely fast, at the expense of the rare, coupled case.

## Integration with CLAD and SAA

The CRPECS is the cornerstone of the **`khora-data`** crate and the ultimate implementation of the **[D]ata** in CLAD.
*   It provides the perfect foundation for **AGDF**, as the SAA's Control Plane can cheaply and frequently alter data layouts by modifying `EntityMetadata`.
*   The **garbage collection** and page compaction processes can themselves be implemented as **Intelligent Subsystem Agents (ISAs)**, operating on a low-priority thread when the system has spare resources, making the ECS itself a living, self-optimizing part of the SAA.