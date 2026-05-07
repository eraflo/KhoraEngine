# Project structure

A **Khora project** is the directory the editor opens with `khora-editor --project <path>`. It bundles the user's scenes, assets, gameplay scripts, and an optional native Rust extension crate.

The Khora **Hub** is the only authoritative producer of new projects: it materialises the layout described below when a user clicks "Create new project". The editor and the SDK consume the same layout — anything not documented here is not part of the contract.

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

The hub creates every folder in this tree, even when empty, so the editor's asset browser can surface the canonical categories from day one.

The first time the editor opens a project, it writes `assets/scenes/default.kscene` (a Main Camera + a Directional Light) so the user has a viable scene to start from. See [`scene_io.rs`](../../../crates/khora-editor/src/scene_io.rs).

### Three tiers of "code"

A Khora project layers three sources of behaviour, each with a different lifecycle:

| Tier | Lives in | Compilation | Hot-reload | Cross-platform |
|------|----------|-------------|------------|----------------|
| 1. **Engine built-ins** | `khora-sdk` (linked into every binary) | n/a — engine is pre-compiled | no | ✅ pre-built per target |
| 2. **Native Rust** | `src/` + `Cargo.toml` (opt-in) | `cargo build --release` | no, requires rebuild | ⚠️ host-only in v1 |
| 3. **Scripts** | `assets/scripts/*.kscript` | none — they're data | ✅ via the project's asset watcher | ✅ universal |

Tier 1 supplies the primitives (`Transform`, `Camera`, `Light`, `Mesh`, ECS plumbing). Tier 2 extends those primitives with custom Rust types when you need raw access to internal APIs or compile-time guarantees. Tier 3 sits on top: gameplay logic expressed as data, hot-reloadable at runtime — no recompile needed to iterate.

You can ship a game with any subset. **Tier 2 is opt-in**: a fresh project from the hub doesn't have a `Cargo.toml` or `src/`. Most games start data-only (tiers 1 + 3) and stay there.

### Adding native code to a project (tier 2)

The hub's home page shows an **"Add Native Code"** button on the card of any project that doesn't yet have a `Cargo.toml`. Clicking it scaffolds:

```text
<project>/
├── Cargo.toml      # depends on khora-sdk = "<engine_version>"
└── src/main.rs     # `fn main() { khora_sdk::run_default() }`
```

The generated `main.rs` is functionally equivalent to the pre-built `khora-runtime` binary — your project still works the same after opt-in. To register custom components, agents, or lanes, edit `src/main.rs` (or split into your own modules) and replace the body with a custom `EngineApp` impl. See [`18_editor.md`](../18_editor.md#build-game) for how the editor's Build Game switches strategy when `Cargo.toml` is present.

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
| `name` | `string` | User input in the hub, sanitised (alphanumerics, `_`, `-`, spaces → `_`) | Human-readable project name. Distinct from the on-disk folder name when sanitisation changed it. |
| `engine_version` | `string` | Selected from the hub's available-engines dropdown | The Khora SDK release the project targets. The editor displays this in the status bar and the command palette footer. |
| `created_at` | `u64` | Unix epoch seconds at hub creation time | Informational. Not used for runtime logic. |

The descriptor type lives in [`hub/src/project.rs`](../../../hub/src/project.rs) (`ProjectDescriptor`) — it is private to the hub today, but the JSON shape is the public contract.

The editor reads `name` and `engine_version` at startup ([`crates/khora-editor/src/main.rs`](../../../crates/khora-editor/src/main.rs), `setup`). Other fields are ignored. Future fields are additive — old editors will keep working.

## Asset extensions

The editor's asset browser categorises files by extension. The mapping lives in [`crates/khora-editor/src/scene_io.rs`](../../../crates/khora-editor/src/scene_io.rs).

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

1. **Creation** — the user picks a name + engine version + parent folder in the hub. The hub calls `project::create_project(...)` which writes the layout above. As of v0.4 the hub also seeds `assets/scripts/main.kscript` (a JSON stub for the future scripting language; safe to ignore today).
2. **Open** — `khora-editor --project <path>` reads `project.json`, builds the project's VFS by scanning `assets/`, arms a filesystem watcher for hot reload, optionally reads the git branch from `.git/HEAD`, and populates `EditorState`.
3. **First open** — the editor writes `assets/scenes/default.kscene` if it doesn't exist.
4. **Edit** — every save mutates files under `assets/`. The editor doesn't touch `project.json` or `src/` after creation. Hot reload picks up disk changes within one frame.
5. **Build** — `Build → Build Game…` in the editor menu runs the asset packer (`khora_io::asset::PackBuilder`) against `<project>/assets/` and stages a runnable output under `<project>/dist/<target>/`. The exact strategy depends on whether the project has opted into native Rust:

    | Project state | Strategy | Result |
    |---|---|---|
    | No `Cargo.toml` (data-only) | **Runtime stamp** | Pre-built `khora-runtime` for the chosen target is copied + renamed |
    | `Cargo.toml` present | **Cargo build** | `cargo build --release --manifest-path <project>/Cargo.toml` runs; the produced binary replaces the runtime stamp |

    Either way, `data.pack` + `index.bin` + `runtime.json` are emitted alongside the binary, and the runtime auto-detects them at startup.

   The runtime stamp path is **trivially cross-platform**: every target's pre-built `khora-runtime` lives in the hub's engine cache (`~/.khora/engines/<version>/runtime/`) and is just a file copy. The cargo path is **host-only in v1** because Rust cross-compilation needs per-target toolchains; running the editor on each target is the simplest workaround until `cross` integration lands.

## What the editor reads from `project.json` today

- ✅ `name` — shown in the brand pill and status bar.
- ✅ `engine_version` — shown in the status bar (`Khora v<version>`) and the command palette footer.
- ⏳ `created_at` — read but ignored.

### WIP / future fields

These are not part of the contract yet but are obvious extensions:

- `description` — long-form project blurb, surfaced in the hub.
- `default_scene` — relative path of the scene to auto-load (currently always `assets/scenes/default.kscene`).
- `default_camera` — entity name to focus on at first open.
- `engine_features` — toggle list to opt into experimental subsystems.

Add a field by extending `ProjectDescriptor` in `hub/src/project.rs` and reading it in `crates/khora-editor/src/main.rs::setup`. The JSON is untyped on read, so older project files keep working.
