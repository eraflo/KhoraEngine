# Serialization

Saving and loading scenes through several strategies behind one file format, chosen
by intent rather than by hand. This page explains *why* a scene has more than one
on-disk encoding and how the engine picks between them. It is an explanation, not a
recipe — for the steps to save and load a scene, see
[How-to: save and load scenes](../how-to/save-and-load-scenes.md).

---

## Why more than one strategy

A scene file has more than one consumer, and they want incompatible things. The
editor wants something human-readable, hand-editable, and stable across years of Git
history. A release build wants something compact. Play mode wants something that
snapshots and restores near-instantly. No single encoding is best at all three.

So Khora serializes through a **[strategy](../reference/glossary.md)** — a concrete
encoding selected by *intent*. The developer states a **`SerializationGoal`**; the
engine maps that goal to a strategy. Choosing the goal is a developer decision;
choosing the strategy is an engine decision. The goals are:

| Goal | Encoded as |
|---|---|
| `HumanReadableDebug` | Definition — a human-readable, hand-editable text encoding |
| `LongTermStability` | Definition — same, chosen for archival robustness |
| `EditorInterchange` | Recipe — compact binary, the editor's working format |
| `SmallestFileSize` | Recipe — same, chosen for size |
| `FastestLoad` | Archetype — a near-`memcpy` binary layout |
| `PortableBinary` | MessagePack — a portable, cross-tool binary encoding |

Four strategies back those six goals. The mapping lives in one place in the
serialization service, so a goal always resolves to the same strategy.

## One service, one file format

Scene save and load is a **service**, not an agent — there are no per-frame
strategies to negotiate, so it sits on the same side of the line as asset loading.
The service exposes `save_world` (take a goal, produce a scene file) and
`load_world` (take a scene file, repopulate the world).

All strategies share **one file format**: a fixed-size header — a magic number, a
format version, the strategy identifier, and the payload length — followed by the
payload. The header is what makes loading symmetric: it records which strategy
produced the payload, so `load_world` dispatches to the matching strategy without
the caller having to know or specify it. The format version is a migration seam for
future on-disk changes.

## How components serialize

The reason adding strategies is tractable is that component serialization is
generated, not hand-written. Deriving the component macro on a type generates a
serialization mirror with encode/decode, the conversions to and from the live type,
and a self-registration so scene loading discovers it with no hand-maintained list.

The mirror exists because GPU handles, runtime caches, and trait objects do not
serialize. Fields the developer marks as skipped are excluded from the mirror and
reconstructed on load — typically by the asset system. Components needing a fully
manual mirror opt out of generation and implement encode/decode by hand. The
registration is the seam: scene loading walks the registry, decodes the right mirror,
converts to the live type, and attaches it to the entity — no string lookups in the
hot path. Maintaining two structs by hand was historically the single biggest source
of serialization bugs, which is exactly why the macro generates the mirror.

## Play-mode snapshots

Pressing Play snapshots the world; pressing Stop restores it. This rides on the same
service: the snapshot is just a `save_world` into memory and the restore a
`load_world` back, fast because the chosen encoding serializes pages with minimal
transformation — a large scene snapshots and restores in milliseconds.

One honest caveat: **physics state is not preserved** across a snapshot. On restore,
the physics engine rebuilds from component data, so velocities and contacts reset to
defaults. This is consistent and predictable; whether to add a goal that captures
physics state is an open question, not a bug.

## Next steps

- [How-to: save and load scenes](../how-to/save-and-load-scenes.md) — save with a
  goal and load a scene with the real service API.
- [Data and the ECS](./ecs.md) — the component model the mirror is generated from.
- [Assets](./assets.md) — how scene references to assets resolve by UUID.
- [Glossary](../reference/glossary.md) — SerializationGoal, strategy, scene file.
