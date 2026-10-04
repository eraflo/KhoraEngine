# Project structure

A **Khora project** is the directory the editor opens with
`khora-editor --project <path>`. It bundles the user's scenes, assets, gameplay
scripts, and an optional native Rust extension crate.

The Khora **Hub** is the only authoritative producer of new projects: it
materialises the layout below when a user creates a project. The editor and the
SDK consume the same layout — anything not documented here is not part of the
contract.

## Folder layout

```text
<name>/
├── project.json           # project descriptor (see "project.json schema")
├── README.md
├── .gitignore             # target/, IDE folders, OS and editor scratch files
├── src/                   # native Rust extensions (created empty; see tier 2)
└── assets/                # runtime data (loaded by the engine)
    ├── scenes/            # *.kscene
    ├── textures/          # png, jpg, jpeg, tga, bmp, hdr
    ├── meshes/            # gltf, glb, obj, fbx
    ├── audio/             # wav, ogg, mp3, flac
    ├── shaders/           # wgsl, hlsl, glsl
    └── scripts/           # Ergon modules, *.erg — main.erg is seeded
```

The hub creates every folder in this tree, even when empty, so the editor's asset
browser can surface the canonical categories from day one.

The first time the editor opens a project, it writes
`assets/scenes/default.kscene` (a Main Camera + a Directional Light) so the user
has a viable scene to start from. See
[`scene_io.rs`](../../../crates/khora-editor/src/scene_io/mod.rs).

Two kinds of file have no folder of their own:

- **Prefabs** (`.kprefab`) are written wherever the author saves them — the
  asset browser's current folder, or the location picked in the save dialog
  when a subtree is saved from the scene tree. A prefab is a scene file of a
  subtree; scenes link to it by its asset id, which survives a move or rename
  made in the editor (the editor freezes it in `.khora/asset-registry.ron`). See [Prefabs](../concepts/prefabs.md).
- **Game saves** have no convention at all. `GameWorld::save_game` returns a
  `SceneFile` (magic bytes `KHORASAV`) and the game decides where its bytes go;
  the engine names no folder and no extension. A save holds only the
  differences from its scene, so it is useless without the scene it names —
  see [Scenes and game saves](../concepts/saves.md).

## Three tiers of code

A Khora project layers three sources of behaviour, each with a different
lifecycle:

| Tier | Lives in | Compilation | Hot-reload | Cross-platform |
|------|----------|-------------|------------|----------------|
| 1. Engine built-ins | `khora-sdk` (linked into every binary) | n/a — engine is pre-compiled | no | pre-built per target |
| 2. Native Rust | `src/` + `Cargo.toml` (opt-in) | `cargo build --release` | no, requires rebuild | host-only in v1 |
| 3. Scripts | `assets/scripts/*.erg` | by the engine, at startup — no toolchain | via the project's asset watcher | universal |

Tier 1 supplies the primitives (`Transform`, `Camera`, `Light`, `Mesh`, ECS
plumbing). Tier 2 extends them with custom Rust types when you need raw access to
internal APIs or compile-time guarantees. Tier 3 sits on top: gameplay logic
written in [Ergon](../concepts/scripting.md), compiled by the engine itself and
hot-reloadable at runtime — no Rust recompile to iterate. A behavior suspended
mid-body when its script is edited resumes in the new code where it can; see
[Scripting](../concepts/scripting.md).

A game can ship with any subset. **Tier 2 is opt-in**: a fresh project from the
hub has no `Cargo.toml` (its `src/` folder is created empty). Most games start
data-only (tiers 1 + 3) and stay there.

### Adding native code (tier 2)

The hub shows an **"Add Native Code"** button on any project without a
`Cargo.toml`. Clicking it scaffolds:

```text
<project>/
├── Cargo.toml      # depends on khora-sdk = "<engine_version>"
└── src/main.rs     # `fn main() { khora_sdk::run_default() }`
```

The generated `main.rs` is functionally equivalent to the pre-built
`khora-runtime` binary. To register custom components, agents, or lanes, replace
its body with a custom `EngineApp` impl.

## `project.json` schema

```json
{
  "name": "MyGame",
  "engine_version": "0.3.0",
  "created_at": 1714659000
}
```

| Field | Type | Source | Meaning |
|-------|------|--------|---------|
| `name` | `string` | Hub input, sanitised (alphanumerics, `_`, `-`, spaces → `_`) | Human-readable name. Distinct from the on-disk folder name when sanitisation changed it. |
| `engine_version` | `string` | Hub's available-engines dropdown | The Khora SDK release the project targets. Shown in the editor status bar and command-palette footer. |
| `created_at` | `u64` | Unix epoch seconds at creation | Informational; not used for runtime logic. |

The descriptor type is `ProjectDescriptor` in
[`crates/khora-hub/src/services/project.rs`](../../../crates/khora-hub/src/services/project.rs) — private to the hub today, but
the JSON shape is the public contract. The editor reads `name` and
`engine_version` at startup
([`crates/khora-editor/src/main.rs`](../../../crates/khora-editor/src/main.rs),
`setup`); other fields are ignored. Future fields are additive — old editors keep
working.

### What the editor reads today

- `name` — shown in the brand pill and status bar.
- `engine_version` — shown in the status bar (`Khora v<version>`) and the command-palette footer.
- `created_at` — read but ignored.

Obvious future additions (not yet part of the contract): `description`,
`default_scene`, `default_camera`, `engine_features`. The JSON is untyped on read,
so older project files keep working.

## Asset extensions

The asset browser categorises files by extension. The mapping is
`asset_type_for_extension` in
[`crates/khora-io/src/asset/index_builder.rs`](../../../crates/khora-io/src/asset/index_builder.rs)
— it is the engine's, not the editor's, so a packed build classifies identically.

| Type | Recognised extensions |
|------|-----------------------|
| Mesh | `.gltf`, `.glb`, `.obj`, `.fbx` |
| Texture | `.png`, `.jpg`, `.jpeg`, `.tga`, `.bmp`, `.hdr` |
| Audio | `.wav`, `.ogg`, `.mp3`, `.flac` |
| Shader | `.wgsl`, `.hlsl`, `.glsl` |
| Material | `.mat`, `.kmat` |
| Scene | `.scene`, `.kscene` |
| Font | `.ttf`, `.otf` |
| Script | `.erg` (Ergon), `.kscript` |
| Prefab | `.kprefab` |

An unknown extension is **not** discarded and not lumped together: the file is
indexed under its own lowercased extension as the type tag, so it shows up in the
browser and a decoder can be added later without re-indexing. Only a file with no
extension at all gets the generic `blob` tag.

## Lifecycle

1. **Creation** — the user picks a name, engine version, and parent folder in the
   hub; the hub writes the layout above. It also seeds
   `assets/scripts/main.erg`, a real Ergon module that compiles and runs —
   it does nothing visible until a behavior it declares is attached to an
   entity.
2. **Open** — `khora-editor --project <path>` reads `project.json`, builds the
   project's VFS by scanning `assets/`, arms a filesystem watcher for hot reload,
   and populates `EditorState`.
3. **First open** — the editor writes `assets/scenes/default.kscene` if absent.
4. **Edit** — every save mutates files under `assets/`. The editor does not touch
   `project.json` or `src/` after creation. Hot reload picks up disk changes
   within one frame.
5. **Build** — `Build → Build Game…` runs the asset packer
   (`khora_io::asset::PackBuilder`) against `<project>/assets/` and stages a
   runnable output under `<project>/dist/<target>/`. The strategy depends on
   whether the project opted into native Rust:

   | Project state | Strategy | Result |
   |---|---|---|
   | No `Cargo.toml` (data-only) | Runtime stamp | The pre-built `khora-runtime` for the target is copied and renamed. |
   | `Cargo.toml` present | Cargo build | `cargo build --release` runs; the produced binary replaces the runtime stamp. |

   Either way, `data.pack` + `index.bin` + `runtime.json` are emitted alongside
   the binary, and the runtime auto-detects them at startup. The runtime-stamp
   path is trivially cross-platform (a file copy from the hub's engine cache); the
   cargo path is host-only in v1.

---

*The `.kscene`, `.pack`, and `.kmat` formats these folders hold are documented in
[File formats](./formats.md). The asset pipeline behind them is the
[Assets](../concepts/assets.md) concept page.*
