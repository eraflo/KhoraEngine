# KhoraEngine Architecture Design (Focus: SAA)

## 1. Goal

This document outlines the initial architectural design choices for KhoraEngine, with a strong focus on enabling the long-term goal of a Symbiotic Adaptive Architecture (SAA). Key principles are modularity, measurability, and adaptability.

## 2. Core Concepts & Runtime Subsystems (Potential ISAs)

The engine will be built around several core concepts and subsystems, designed with the potential to become Intelligent Subsystem Agents (ISAs) within the SAA framework.

*   **Scene:** Represents the active collection of entities, their components, and associated resources that constitute the current game world state (e.g., a level, a menu). It is the primary context for most runtime operations.
*   **Entity Component System (ECS):** The core data management paradigm.
    *   **World:** The container holding all entities and component data for the currently active *Scene*.
    *   **Entities:** Unique identifiers representing objects in the scene.
    *   **Components:** Raw data associated with entities (e.g., Transform, MeshRenderer, RigidBody, AIState). Designed for Data-Oriented Design (DOD).
    *   **Systems:** Logic that operates on entities possessing specific sets of components (e.g., PhysicsSystem, RenderSystem, AISystem).
*   **Scene Management:** Responsible for the lifecycle of Scenes.
    *   Coordinates loading scene data (using the Asset System) into the ECS World.
    *   Manages unloading scenes and cleaning up resources.
    *   Handles transitions between different scenes.
    *   Communicates scene status changes (e.g., via the Message Bus).
*   **Renderer:** Responsible for drawing the active Scene (based on data in the ECS World) to the screen. Will track GPU/VRAM usage and potentially offer multiple rendering strategies. (Potential ISA)
*   **Input System:** Manages user input and translates them into engine events (often distributed via the Message Bus).
*   **Asset System:** Handles loading, unloading, and management of game resources (scene files, meshes, textures, audio, materials). Will track memory/disk usage and potentially implement adaptive loading strategies. (Potential ISA)
*   **Physics System:** Simulates physical interactions between entities in the Scene (using data from the ECS World). Will track CPU usage and potentially offer different simulation fidelities. (Potential ISA)
*   **Audio System:** Manages playback and spatialization of sound within the Scene context. Will track CPU/memory usage. (Potential ISA)
*   **In-Engine UI System:** Renders user interfaces within the game viewport, often tied to the active Scene state. Performance will be tracked.
*   **Networking System:** Manages communication for multiplayer scenarios, synchronizing the Scene state across clients/server. Will track bandwidth/latency/CPU. (Potential ISA)
*   **Animation System:** Handles playback and blending of animations for entities within the Scene. Will track CPU/GPU cost. (Potential ISA)
*   **AI System:** Manages the behavior of non-player entities within the Scene. Will track CPU usage and potentially offer adaptive strategies. (Potential ISA)

## 3. Communication Strategy

To foster decoupling and enable future SAA interactions, a **combined communication strategy** will be employed:

*   **Entity Component System (ECS) World:**
    *   **Role:** Acts as the central repository for the *data state* of the currently active **Scene**. Subsystems query and modify component data here.
    *   **Benefits:** Promotes Data-Oriented Design (DOD), enables efficient iteration, aligns with performance goals.
*   **Message/Event Bus:**
    *   **Role:** Handles asynchronous communication, events, and commands *related to* the scene or system interactions.
        *   **Events:** Notifications like `InputReceived`, `CollisionOccurred` (often referencing specific entities within the scene), `SceneLoading`, `SceneLoaded`, `AssetLoaded`, `NetworkPacketReceived`.
        *   **Commands:** Used by the future DCC to instruct ISAs, or for triggering actions like `LoadSceneCommand("level_2")`, `SpawnEntityCommand`.
        *   **Status Reporting (Optional):** ISAs might use messages to asynchronously report status/metrics to the DCC.
    *   **Benefits:** Strong decoupling. Facilitates the agent-based model of SAA. Allows reacting to occurrences without direct dependencies.

## 4. Context & Metrics Flow

A fundamental aspect of SAA is context awareness. Performance metrics (CPU timings, GPU timings, memory usage, VRAM usage, network latency, etc.) and resource usage will be collected *within* each relevant subsystem. This data will initially be displayed for debugging and analysis. In later SAA phases, this contextual information will be routed (likely via the message bus or a dedicated reporting mechanism) to the DCC for analysis and decision-making.

## 5. Conclusion

This architecture, based on well-defined subsystems, an ECS for shared state, and a message bus for decoupled communication, provides a solid foundation for building the engine features while keeping the path towards the Symbiotic Adaptive Architecture open and viable.