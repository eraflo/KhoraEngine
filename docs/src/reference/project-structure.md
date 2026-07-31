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
├── .gitignore             # target/ and *.lock
├── src/                   # native Rust extensions (compiled into the game)
└── assets/                # runtime data (loaded by the engine)
    ├── scenes/            # *.kscene
    ├── textures/          # png, jpg, jpeg, tga, bmp, hdr
    ├── meshes/            # gltf, glb, obj, fbx
    ├── audio/             # wav, ogg, mp3, flac
    ├── shaders/           # wgsl, hlsl, glsl
    └── scripts/           # gameplay scripts (data, hot-reloadable)
```

The hub creates every folder in this tree, even when empty, so the editor's asset
browser can surface the canonical categories from day one.

The first time the editor opens a project, it writes
`assets/scenes/default.kscene` (a Main Camera + a Directional Light) so the user
has a viable scene to start from. See
[`scene_io.rs`](../../../crates/khora-editor/src/scene_io.rs).

## Three tiers of code

A Khora project layers three sources of behaviour, each with a different
lifecycle:

| Tier | Lives in | Compilation | Hot-reload | Cross-platform |
|------|----------|-------------|------------|----------------|
| 1. Engine built-ins | `khora-sdk` (linked into every binary) | n/a — engine is pre-compiled | no | pre-built per target |
| 2. Native Rust | `src/` + `Cargo.toml` (opt-in) | `cargo build --release` | no, requires rebuild | host-only in v1 |
| 3. Scripts | `assets/scripts/*.kscript` | none — they are data | via the project's asset watcher | universal |

Tier 1 supplies the primitives (`Transform`, `Camera`, `Light`, `Mesh`, ECS
plumbing). Tier 2 extends them with custom Rust types when you need raw access to
internal APIs or compile-time guarantees. Tier 3 sits on top: gameplay logic
expressed as data, hot-reloadable at runtime — no recompile to iterate.

A game can ship with any subset. **Tier 2 is opt-in**: a fresh project from the
hub has no `Cargo.toml` or `src/`. Most games start data-only (tiers 1 + 3) and
stay there.

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
[`hub/src/project.rs`](../../../hub/src/project.rs) — private to the hub today, but
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

The editor's asset browser categorises files by extension. The mapping lives in
[`crates/khora-editor/src/scene_io.rs`](../../../crates/khora-editor/src/scene_io.rs).

| Type | Recognised extensions |
|------|-----------------------|
| Mesh | `.gltf`, `.glb`, `.obj`, `.fbx` |
| Texture | `.png`, `.jpg`, `.jpeg`, `.tga`, `.bmp`, `.hdr` |
| Audio | `.wav`, `.ogg`, `.mp3`, `.flac` |
| Shader | `.wgsl`, `.hlsl`, `.glsl` |
| Material | `.mat`, `.kmat` |
| Scene | `.scene`, `.kscene` |
| Font | `.ttf`, `.otf` |

Files with unknown extensions are still scanned but classified as generic.

## Lifecycle

1. **Creation** — the user picks a name, engine version, and parent folder in the
   hub; the hub writes the layout above. It also seeds
   `assets/scripts/main.kscript` (a stub for the future scripting language; safe
   to ignore today).
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
