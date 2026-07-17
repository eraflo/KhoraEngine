# File formats

The on-disk formats Khora reads and writes: the `.kscene` scene file, the `.pack`
asset archive, and the `.kmat` material file. This page is structure and facts —
for *why* the formats are shaped this way see
[Serialization](../concepts/serialization.md) and [Assets](../concepts/assets.md);
to *save and load* a scene see
[Save and load scenes](../how-to/save-and-load-scenes.md).

## `.kscene` — scene file

A `.kscene` file is a fixed-size header followed by a single payload. The header
records which serialization strategy produced the payload, so loading is symmetric
without prior format knowledge.

### Header layout

The header is a fixed `SceneHeader` (defined in
`crates/khora-core/src/scene/format.rs`). Its size is **49 bytes**
(`SceneHeader::SIZE = 8 + 1 + 32 + 8`):

| Field | Type | Bytes | Notes |
|---|---|---|---|
| `magic_bytes` | `[u8; 8]` | 8 | Always `"KHORASCN"` (`HEADER_MAGIC_BYTES`). |
| `format_version` | `u8` | 1 | Header/format version. Current writers emit `1` (`CURRENT_SCENE_VERSION`). |
| `strategy_id` | `[u8; 32]` | 32 | Null-padded UTF-8 strategy ID, e.g. `"KH_RECIPE_V1"`. |
| `payload_length` | `u64` | 8 | Length of the payload that follows, in bytes (little-endian). |

The payload immediately follows the header. `SceneFile::to_bytes` /
`SceneFile::from_bytes` serialize and parse the whole file; the header is written
by direct byte manipulation (not serde) because it is fixed-layout and
performance-critical. `from_bytes` returns `SceneFileError::InvalidMagicBytes` or
`SceneFileError::TooShort` on a malformed file.

### Strategies and goals

The payload encoding is chosen by a `SerializationGoal`, **not** by file
extension. The `strategy_id` in the header records which strategy produced the
payload. There are **four** strategies, each identified by a versioned string ID:

| Strategy | `strategy_id` | Payload encoding | Character |
|---|---|---|---|
| Definition | `KH_DEFINITION_RON_V1` | RON (text) | Human-readable, diffable. |
| Recipe | `KH_RECIPE_V1` | Binary command list | Compact, editor interchange. |
| Archetype | `KH_ARCHETYPE_V1` | Binary page layout | Fastest load; play-mode snapshots. |
| MessagePack | `KH_MESSAGEPACK_V1` | MessagePack | Portable, schema-less, cross-language. |

`SerializationGoal` (in `khora-core::scene`) has **six** variants. The goal → strategy
mapping is performed in `SerializationService::save_world`:

| `SerializationGoal` | Strategy |
|---|---|
| `HumanReadableDebug` | Definition (`KH_DEFINITION_RON_V1`) |
| `LongTermStability` | Definition (`KH_DEFINITION_RON_V1`) |
| `SmallestFileSize` | Recipe (`KH_RECIPE_V1`) |
| `EditorInterchange` | Recipe (`KH_RECIPE_V1`) |
| `FastestLoad` | Archetype (`KH_ARCHETYPE_V1`) |
| `PortableBinary` | MessagePack (`KH_MESSAGEPACK_V1`) |

On load, `SerializationService::load_world` reads the `strategy_id` from the
header and dispatches to the matching strategy — the goal is irrelevant at load
time. A migration seam (`migrate_payload`) is wired for future format bumps; no
migrations are registered today (version `1` is the only scene format).

> Choosing a *goal* is a developer decision; choosing a *strategy* is an engine
> decision. The four strategies implement the `SerializationStrategy` trait in
> `khora-data::scene`.

## `.pack` — asset archive

In release builds, a project's assets are bundled into a **two-file** layout
written by `khora_io::asset::PackBuilder`, staged under
`<project>/dist/<target>/`:

- `data.pack` — the concatenation of every asset's bytes.
- `index.bin` — a bincode-encoded `Vec<AssetMetadata>` mapping each asset's UUID
  to its location inside `data.pack`.

Both files are needed together. The split keeps the loader's I/O trivially
zero-copy and lets a tool inspect either file independently.

### `data.pack` layout

```
data.pack
┌──────────────────────────────────────────────────┐  offset 0
│ Header (16 bytes)                                 │
│   ─ Magic:          "KHORAPK\0"  (8 bytes)        │
│   ─ format_version: u32 LE       (4 bytes) = 1    │
│   ─ asset_count:    u32 LE       (4 bytes)        │
├──────────────────────────────────────────────────┤  offset 16
│ asset 0 bytes                                     │
│ asset 1 bytes                                     │
│ …                                                 │
│ asset N-1 bytes                                   │
└──────────────────────────────────────────────────┘
```

The 16-byte header lets `PackLoader::new` fail fast on a wrong/renamed file (bad
magic), a pack from a future engine (`format_version` mismatch), or an `index.bin`
that has drifted out of sync (`asset_count` cross-check). There are no checksums,
no per-asset framing, and no padding — every byte after the header is asset
payload. Asset offsets recorded in `index.bin` are **relative to the asset
region** (the first asset is at 0); the loader adds the header size when seeking.

The constants are public: `PACK_MAGIC`, `PACK_FORMAT_VERSION`, `PACK_HEADER_SIZE`
(re-exported through `khora-sdk`).

### `index.bin` layout

`bincode(Vec<AssetMetadata>)`, encoded with `bincode::config::standard()` so
future additions to `AssetMetadata` keep older files readable. Each
`AssetMetadata` carries:

| Field | Meaning |
|---|---|
| `uuid` | `AssetUUID` — the asset's stable identity: `AssetUUID::new_v5(forward_slash_rel_path)` by default, or the frozen value the [identity registry](#asset-identity-registry) holds for that path. |
| `asset_type_name` | Canonical type tag (`"texture"`, `"mesh"`, `"material"`, `"script"`, …). |
| `dependencies` | `Vec<AssetUUID>` — assets this one references (deduplicated, sorted). |
| `variants` | `HashMap<String, AssetSource>` — the `"default"` variant points into `data.pack` as `Packed { offset, size }`. |
| `tags` | `Vec<String>`. |

### Determinism

`IndexBuilder` sorts asset paths lexicographically (forward-slash relative path)
before `PackBuilder` streams them, so two consecutive packs of the same `assets/`
directory are byte-identical. Each asset's UUID is resolved the same way in both
modes: the frozen entry from the [identity registry](#asset-identity-registry)
if one exists, otherwise the default `AssetUUID::new_v5(forward_slash_rel_path)`.
Because dev (`IndexBuilder::with_registry`) and release (`PackBuilder`, which loads
the same registry) resolve through it identically, a UUID is **the same in dev mode**
(loose files + in-memory index) **and release mode** (packed file + on-disk
`index.bin`) by construction. Game code carrying an `AssetHandle<T>` works in either
mode unchanged.

## Asset identity registry

An asset's `AssetUUID` must survive a rename or move so that references stored as
raw UUID bytes — `MeshRef::Asset`, `MaterialRef`, `.kmat` texture slots — never
break. The `<project_root>/.khora/asset-registry.ron` file decouples identity from
path: it freezes a stable UUID for a relative path (`AssetIdRegistry` in
`crates/khora-io/src/asset/id_registry.rs`).

**Lazy freeze.** An asset that has never been renamed has **no** registry entry and
keeps its `AssetUUID::new_v5(rel_path)` default, so pre-registry projects and tests
are unaffected. The first time an asset is renamed or moved in the editor, its
*current* UUID is frozen into the registry — so it keeps that UUID forever,
regardless of any future path change. A rename therefore rewrites nothing: scenes,
prefabs, and `.kmat` files on disk and the open scene all keep resolving through the
unchanged UUID.

**Format.** RON, one `(uuid, path)` entry per asset, **sorted by UUID**. Because the
UUID is immutable, adding, renaming, or deleting an asset each touch a single line,
so two branches that rename *different* assets produce non-overlapping diffs that git
3-way-merges cleanly (a real conflict arises only when the same asset is renamed on
both branches). The file is written **atomically** (temp file + rename) so a crash
mid-write cannot corrupt it.

**Placement.** The registry lives at the **project root**, a sibling of `assets/`, so
it is never scanned, watched, or packed — the scanner, the filesystem watcher, and
the packer are all rooted at `assets/`. The editor (`ProjectVfs`) is the **only**
writer; the read side (`IndexBuilder::with_registry`, the runtime, and `PackBuilder`)
resolves through it, which is what makes dev and release agree on identity.

## `.kmat` — material file

A `.kmat` is a material asset referenced from an entity via
`MaterialRef::Asset(uuid)` and resolved through the VFS. The recognised
extensions for the material category are `.kmat` and `.mat`
(`IndexBuilder::asset_type_for_extension` maps both to the type tag `"material"`).

The on-disk form is **RON** of a type-tagged tree:

```ron
{
    "type_name": "StandardMaterial",
    "material": { /* the concrete material fields */ },
}
```

`type_name` selects a `MaterialRegistration` from an open, inventory-based
registry (`khora-data::ecs::components::material_registry`); the `material`
sub-value is decoded by that registration. Four material types register by
default:

| `type_name` | Type | Workflow |
|---|---|---|
| `StandardMaterial` | PBR metallic-roughness | `base_color`, `metallic`, `roughness`, optional texture maps (base-color, metallic-roughness, normal, emissive, occlusion). |
| `UnlitMaterial` | Unlit | Flat color, no lighting. |
| `EmissiveMaterial` | Emissive | Self-illuminating. |
| `WireframeMaterial` | Wireframe | Edge rendering. |

A material's texture references contribute its dependency list in the pack index:
the `dependencies` of a `.kmat` are the `AssetUUID`s of the textures it references
(see `khora_io::asset::dependencies`). Custom material types become serializable
by registering their own `MaterialRegistration` (the `#[derive(Material)]` macro
generates it). See [Materials and lighting](../how-to/materials-and-lighting.md).

---

*Rationale: [Serialization](../concepts/serialization.md) ·
[Assets](../concepts/assets.md). Tasks:
[Save and load scenes](../how-to/save-and-load-scenes.md) ·
[Load assets](../how-to/load-assets.md).*
