---
name: load-scene
description: Loads, saves, or serializes a scene in a Khora game via the SDK scene API. Use when persisting world state, loading levels, or working with prefabs.
---

# Load / save a scene

Scenes are handled through the SDK's serialization surface — `SceneFile` + `SerializationGoal`.

## Concepts
- `SerializationGoal` picks the strategy/trade-off: Editor Interchange, Fastest Load, Smallest File,
  Human Readable, Long-Term Stability, Portable Binary.
- The **editor** (`cargo run -p khora-editor` in the engine repo) authors scenes (`.kscene`) and prefabs
  (`.kprefab`) visually; your game loads them at runtime.
- Subtree helpers exist for prefabs (`instantiate_subtree` / `serialize_subtree`).

## Steps
1. Decide when the scene loads (startup, level transition) and from where (packed asset vs. file).
2. Load via the `AssetService` / scene API exposed by the SDK; instantiate into your `GameWorld`.
3. To persist, serialize with the appropriate `SerializationGoal`.

## Rules
- Treat scene/asset bytes as untrusted input — handle the `Result`, never `unwrap()`.
- Reference assets by handle; don't inline raw data.

For shipping packed scenes, see [`../pack-and-ship/SKILL.md`](../pack-and-ship/SKILL.md). Verify by loading
the scene and confirming entities appear.
