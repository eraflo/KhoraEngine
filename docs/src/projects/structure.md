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

### `src/` vs `assets/scripts/`

These are deliberately separate roles:

| Location | What lives here | When it runs |
|----------|-----------------|--------------|
| `src/` | Native Rust code: custom components, agents, traits, anything that wants compile-time guarantees and access to internal Khora APIs. | Compiled into the game binary at build time. |
| `assets/scripts/` | Gameplay scripts treated as **data**. Today these can be plain text/JSON used by your custom components; the engine roadmap includes a custom scripting language with hot-reload and live-edit. | Loaded at runtime by an asset loader, not compiled. |

You can ship a game with one, the other, or both. There is no overlap.

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

1. **Creation** — the user picks a name + engine version + parent folder in the hub. The hub calls `project::create_project(...)` which writes the layout above.
2. **Open** — `khora-editor --project <path>` reads `project.json`, scans `assets/`, optionally reads the git branch from `.git/HEAD`, and populates `EditorState`.
3. **First open** — the editor writes `assets/scenes/default.kscene` if it doesn't exist.
4. **Edit** — every save mutates files under `assets/`. The editor doesn't touch `project.json` or `src/` after creation.
5. **Build** — out of scope today; future work will compile `src/` into the game binary and bundle `assets/` next to it.

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
