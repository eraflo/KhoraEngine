# Editor

The editor application — panels, gizmos, play mode, scene I/O.

- Document — Khora Editor v1.0
- Status — Authoritative
- Date — May 2026

---

## Contents

1. What the editor is
2. Workspace anatomy
3. Modes
4. Play mode
5. Scene I/O
6. Asset browser
7. Scripts
8. Gizmos and selection
9. Build Game
10. The Control Plane
11. For game developers
12. For engine contributors
13. Decisions
14. Open questions

---

## 01 — What the editor is

`khora-editor` is a separate binary built on the SDK. It opens a project (a folder containing `.kscene` files and assets), authors scenes through ECS-aware panels, and previews them with **play mode** — a one-button switch between editing and full simulation.

The editor is not a separate engine. It uses the same agents, lanes, and ECS as a shipping game. What Play changes is the *engine mode* — scripts run only while the game does — the *simulation clock*, and what the panels do on top of the world.

The visual language — colors, typography, panels, voice — is documented in [Editor design system](../design/editor.md). This chapter covers the **architecture**, not the look.

## 02 — Workspace anatomy

```
+--------------------------------------------------------------+
| Title bar — brand, project name, window controls            |
+----+----------------------------------------------+----------+
|    |                                              |          |
| Sp |               Viewport                       | Inspect  |
| in |                                              | -or      |
| e  |                                              |          |
|    |                                              |          |
+----+----------------------------------------------+----------+
| Bottom dock: Assets · Console · Problems · GORNA stream     |
+--------------------------------------------------------------+
| Status bar — engine state, FPS, build status                |
+--------------------------------------------------------------+
```

| Region | Purpose |
|---|---|
| **Title bar** | Brand, project name, window controls |
| **Spine** | Mode rail (Scene, Control Plane) |
| **Hierarchy** | Tree of entities, left of the viewport |
| **Viewport** | The 3D scene with floating gizmos |
| **Inspector** | Components of the selected entity, right of the viewport |
| **Bottom dock** | Assets browser, console, script problems, GORNA stream |
| **Status bar** | Engine state, FPS, build status, GORNA pulse |

Full anatomy and pixel-level layout in [Editor design system](../design/editor.md).

## 03 — Modes

Two workspaces ship today, switched from the Spine:

| Mode | Purpose |
|---|---|
| **Scene** | 3D editing (default) |
| **Control Plane** | Engine telemetry and GORNA stream |

Each mode has its own panel layout. We commit to opinionated defaults — Unity and Unreal let you arrange panels freely, and most users keep the defaults forever. We pick the layouts users won't want to change. Further authoring workspaces (2D canvas, node graph, animation, shader graph) will be added as they are built.

## 04 — Play mode

<div class="kp-figure-frame">

{{#include ../images/editor/play-modes.svg}}

</div>

When you press **Play**:

1. The current world is serialized using `SerializationGoal::FastestLoad` (a schema-bound snapshot), in memory.
2. The editor's `PlayMode` (UI state) becomes `Playing`.
3. The engine mode becomes `Playing`: the script agent starts, and every scripted entity arrives fresh and runs `OnSpawn`.
4. The simulation clock scale goes to `1.0`.
5. The play camera takes over from the editor camera.

**Pause** keeps the engine mode at `Playing` and sets the clock scale to `0.0`: scripts still take their turns, with a delta of zero, so nothing that counts time moves.

When you press **Stop** — from playing or paused:

1. The snapshot is restored with `replace_world`: atomic, every persistent id kept, every runtime `EntityId` fresh — the selection is cleared.
2. The editor's `PlayMode` becomes `Editing`, the engine mode the editor's own, `Custom("editor")`, and the clock scale `0.0`.
3. The editor camera resumes.

Stop is not a load of a game save: the next Play is a fresh start again, with `OnSpawn`, never `OnLoad`. A component whose schema the engine cannot trace whole is not guarded by the snapshot's fingerprint, so a world holding one is snapshotted as a compact record instead. See [Serialization](../concepts/serialization.md).

| Aspect | Editing | Playing | Paused |
|---|---|---|---|
| Engine mode | `Custom("editor")` | `Playing` | `Playing` |
| Scripts | do not run | run | run, delta `0` |
| Other agents | all | all | all |
| Simulation clock | `Time::scale == 0.0` — no fixed sub-steps run | `1.0` | `0.0` |
| Camera | Editor camera (free orbit) | Scene cameras (active ones) | Scene cameras |
| Input | Editor input (gizmos, selection) | Game input (player controls) | Game input |
| ECS | Mutable — user edits directly | Snapshot taken — original world preserved | as Playing |

> **Two things cross from the editor into the engine, both written every frame:**
> the clock's **scale** and the **engine mode**. `PlayMode` itself — buttons,
> panel visibility — stays in the editor.
>
> The scale makes the editor one caller among several: at `0.0` the fixed-step
> accumulator never fills, so no body integrates and no script timer counts
> down, while rendering carries on at real time — a pause menu, a cutscene and a
> slow-motion hit write the same number.
>
> The mode scopes agents: the script agent is registered for `Playing` only
> (`DccService::register_agent_for_mode`), so the editor's own mode is a world
> nobody's gameplay runs in. A shipped game starts in its
> `EngineApp::initial_mode` — `Playing` by default.

> **Physics state is not preserved across play mode.** Velocities, contacts, and sleep state are reset on restore. The ECS components are restored exactly; the physics world rebuilds from those components.

## 05 — Scene I/O

The editor uses `SerializationService` for all scene operations. As of v0.4 it goes through `ProjectVfs` (the editor's wrapper around `khora_io::AssetService` + `AssetWatcher`), so saves and loads inside the project use the same UUID identity that a release pack would:

| Action | What happens |
|---|---|
| **Open project** | `ProjectVfs::open` recursively scans `<project>/assets/`, builds the in-memory UUID index, registers all decoders, arms a filesystem watcher. The asset browser reads off the resulting VFS. |
| **New scene** | Editor creates an empty world, ready for editing |
| **Save scene** | Encoded with `SerializationGoal::EditorInterchange` (compact, records by name), prefab instances written as [links](#prefabs), and written via `AssetWriter` so the new file enters the VFS index immediately |
| **Save scene as** | Same, with a new path. Out-of-project paths fall back to raw `std::fs` and log a warning. |
| **Load scene** | Double-click a `.kscene` in the asset browser, or File → Open. Project-internal paths route through `AssetService::load_raw` (UUID-keyed); prefab links expand from the project's prefabs. A file that cannot be loaded leaves the open scene as it was. |
| **Play / Stop** | World snapshot + restore via `SerializationService` with `FastestLoad` — a schema-bound snapshot, in memory. |

Scene files are compact-binary in development today. JSON text for diffing is available through `SerializationGoal::HumanReadableDebug` — not yet wired to a menu.

## 06 — Asset browser

The Assets panel in the bottom dock is a real file explorer over `<project>/assets/`, not just a read-only list. It reads off the same `ProjectVfs` the rest of the editor uses, so every operation goes through the identity registry — renaming or moving an asset here **does not break its references** (see [Scene I/O](#05--scene-io) and [File formats — asset identity registry](formats.md#asset-identity-registry)).

| Interaction | Result |
|---|---|
| **Left-click** | Selects a tile. |
| **Double-click** | Opens / activates it (a `.kscene` loads, other types open). |
| **Right-click a file tile** | Context menu: Open, Reveal in Explorer, Rename (inline), Duplicate, Delete, plus "Assign to selected" for materials. |
| **Right-click a folder** | New Folder, New Script, Rename, Delete, Reveal. |
| **Right-click empty space** | New Folder, New Script, Reveal Current Folder. |
| **Header buttons** | More (New Folder, New Script, Reveal Project in Explorer), Trash (delete selected), Filter (category menu). |

Empty folders are shown, so **New Folder** produces a usable target immediately. **Delete goes to the OS recycle bin** (via the `trash` crate), so a mistaken delete is reversible from the system trash rather than lost.

**Drag & drop.** Drag a tile onto a folder in the tree to **move** it (the move is mediated by the editor, so the identity registry freezes the UUID and references survive). Drag a tile into the **3D viewport** to instantiate it: a mesh spawns an entity at the drop point, a prefab places a linked instance, a scene loads, and a texture or material assigns to the selected entity.

### Prefabs

A `.kprefab` holds an entity and everything under it; an instance placed in a
scene stays **linked** to it. See [Prefabs](../concepts/prefabs.md) for the model
and [Work with prefabs](../how-to/work-with-prefabs.md) for the steps.

| Action | How |
|---|---|
| Make a prefab | Scene Tree → right-click an entity → **Save as Prefab…**, or drag the entity onto an Asset Browser folder |
| Place an instance | double-click the prefab, drag it into the viewport (at the root), or onto a Scene Tree entity (as its child) |
| Duplicate an instance | another instance of the same prefab |
| See overrides | the Inspector's instance band, the dot on each overriding field, the note on each card |
| Revert | the arrow at a field's end, right-click → Revert to prefab, a card's revert icon, **Revert all** |
| Apply | right-click → Apply to prefab, a card's apply icon, **Apply all** — every other open instance follows at once |

<div class="kp-figure-frame">

{{#include ../images/editor/inspector-prefab.svg}}

</div>

> **Rename/move only inside the editor.** The registry freeze happens because the editor mediates the file operation. Renaming or moving an asset from a shell or `git` bypasses that path — see [Troubleshoot](../how-to/troubleshoot.md#assets--vfs).

## 07 — Scripts

The open project's Ergon scripts live under `<project>/assets/scripts/` and are
part of the editor from the moment the project opens — not only once Play is
pressed.

**One folder.** A script is an `.erg` file under `assets/scripts/` — the folder
and the extension spelled exactly so, lower case, on every system. Its module
is its path from there (`scripts/ai/guard.erg` is `ai/guard.erg`), and every
`import` is written from there too. An `.erg` anywhere else — or one spelled
`.ERG` — is not a script: it is never compiled, and the Problems tab lists it
with what to do (move it, or rename it).

**Mounted with the project.** Opening a project mounts its `assets/` folder
(`khora_sdk::scripts::mount`); opening another unmounts the first, closing
unmounts. Every script is compiled and watched: a save recompiles its module,
while editing as while playing. What the scripts *declare* — components,
behaviors, systems — registers at once, so the Inspector knows the project's
components before anything runs. What they *compiled to* waits for Play: no
script runs while you edit ([Scripts run only while the game
runs](../concepts/scripting.md#scripts-run-only-while-the-game-runs)).

**New Script.** *New Script* — in the Assets panel's empty-space, folder and
*More* menus — makes a script from the editor. The name comes first: a Script
tile appears in the first slot with a name field, and nothing is written until
it is committed (Enter, or a click elsewhere); Escape makes nothing. The name
typed becomes the behavior's, as an Ergon name — `my player` is `MyPlayer` —
and the tile's thumbnail shows the file it will write before you commit. A name
already taken gets a number (`Camera2`): one another `.erg` of the project uses
as its file name, one another script declares inside it — `stats.erg` declaring
`component Health` makes `health` give `Health2` — or one the engine keeps: a
component's such as `Camera`, an asset kind's such as `Sound`, an event's it
raises such as `Touched`. The file
goes in the current folder when that is `scripts/` or under it, otherwise in
`scripts/` — the folder the project's scripts are compiled from — and the panel
opens there. It starts as a behavior that compiles, its parts commented, and an
existing file is never written over. Once written, it is selected, and the
watch compiles it like any save; double-click opens it in the system's editor
for `.erg` files.

**Problems.** The **Problems** tab of the bottom dock lists what is wrong with
the project's scripts, grouped by module, each row `line:col  message` with its
note:

| Source | Severity | What |
|---|---|---|
| Placement | error | an `.erg` outside `scripts/`, or spelled `.ERG`: not compiled — the note says where to move it, or what to rename (a `Scripts/` folder, a `.ERG` extension) |
| Compile | error / warning | a module's diagnostics, located in the module they are in — warnings of a module that compiled too |
| Registration | error | a component declared by two modules: the one refused, the one keeping it |
| Registration | warning | a component no module declares any more — kept with its data |
| Running | error | a faulted instance: its entity, its behavior, the reason |
| Running | error | an instance whose module is gone — its file deleted, moved out of `scripts/`, or its project closed: it does not run, and keeps its fields until the module comes back |
| Running | warning | a body still unfinished past the watchdog's warning limit |
| Running | warning | an event dropped: its payload did not fit the event (sent by the engine, or queued before an edit) — one line per entity, behavior and event |

<div class="kp-figure-frame">

{{#include ../images/editor/problems-tab.svg}}

</div>

A row with a file opens the `.erg` in the system's editor; a running problem
selects its entity. The tab's title counts errors and warnings
(`Problems · 2 errors · 1 warning`), and each `.erg` tile in the Assets panel
wears a badge: the error glyph and how many errors, or — with warnings only —
the warning glyph and how many; never the colour alone — an `.erg` outside
`scripts/` wears one error. A module's problems are replaced each time it
compiles: a clean compile clears them; moving a misplaced file into `scripts/`
clears its row.

**Declared components in the Inspector.** A component a script declares is a
card like a Rust component's, read from its declared fields rather than through
JSON: its fields in declaration order, each drawn by its Ergon type.

| Type | Widget |
|---|---|
| `bool` | checkbox |
| `int`, `float` | scrubber — an `int` stays whole |
| `Duration` | scrubber in seconds, `s` after it |
| `Angle` | scrubber in degrees, `deg` after it — stored in radians |
| `string` | text field |
| `Vec2`/`Vec3`/`Vec4`, `Quat`, `Color` | one scrubber per axis; a colour swatch |
| `Entity` | a slot naming the entity — drop one from the Hierarchy |
| `T?` | the value, or `none` with **Set**; right-click → *Set to none* |
| `T[]` | a row per item, **+ Add item**; right-click an item to move or remove it |
| struct | a nested block, a row per field |

<div class="kp-figure-frame">

{{#include ../images/editor/inspector-declared.svg}}

</div>

A field's [attributes](./ergon.md#field-attributes) change how its row reads,
never what it holds:

| Attribute | On the row |
|---|---|
| `[Header("Combat")]` | a section title above it, small capitals |
| `[Tooltip("…")]` | said by hovering a number's label, or the control of any other field |
| `[Range(0, 100)]` | the same scrubber, an edit held inside the range — the keyboard's steps too; a thin gauge under the row. A value a script or a file wrote outside is shown as it is: its gauge turns amber, hovering it says why, and the first edit brings it back inside |

<div class="kp-figure-frame">

{{#include ../images/editor/inspector-field-attributes.svg}}

</div>

Under the card's title one quiet line says where it comes from — the `.erg`
declaring it — and the header's tag names the part it plays: `script`,
`behavior`, `state`, `system`. An edit writes the row only when the field takes
the value; a value it refuses is logged and the row stays as it was.

| Situation | What the Inspector shows |
|---|---|
| **+ Add Component** | Rust components and the project's declared components and behaviors, grouped by domain, a script's entry marked with its `.erg` — never a behavior's state, a system's fields, or what the entity already holds |
| A behavior removed | its state companion goes with it |
| **Playing** | each behavior's state companion (`Guard#state`: state, timers, pending body, fault) as a read-only card; edits change live values, and Stop restores the snapshot |
| Editing | no state companion — there is no run to show |
| A system | its one entity is listed in the Hierarchy as `system:Name`; its card is read-only |
| No module declares it any more | the card stays, flagged in amber, editable and removable — its data is kept until you decide |
| A prefab instance | declared fields are compared with the prefab's field by field: the same dots, reverts and applies as a Rust component's. The prefab's values are read as the scripts declare the component now, so a field added since the prefab was saved is no override |

## 08 — Gizmos and selection

The viewport has floating gizmos for the selected entity:

- **Move** — three axis arrows.
- **Rotate** — three rings, one per axis.
- **Scale** — three axis arms with end cubes.

Selection is tracked in `EditorState.selection` (a set — multi-select is supported), with `EditorState.inspected` naming the one the Inspector shows. Clicking an entity in the hierarchy or the viewport sets it; `Esc` clears it. Selected entities get a 2 px gold inner-stroke outline (1 px dark outer stroke), visible against any background.

Numeric fields in the Inspector are draggable scrubbers — drag horizontally to change a value, modifier keys for precision. No spinner buttons.

## 09 — Build Game

`Build → Build Game…` packages the open project for a target OS. The editor picks one of two **strategies** automatically, based on the presence of `<project>/Cargo.toml`:

### Strategy A — Runtime stamp (default, data-only projects)

Used when the project has no `Cargo.toml` — the typical state for a fresh project from the hub.

1. **Pack** — `khora_io::asset::PackBuilder` walks `<project>/assets/` (sorted by forward-slash relative path), resolves each file's UUID through the project's identity registry (a frozen entry if one exists, else the `AssetUUID::new_v5(rel_path)` default — the same registry the editor's dev VFS reads, so dev and release UUIDs match by construction; see [File formats — asset identity registry](formats.md#asset-identity-registry)), and writes the two-file release layout: `data.pack` (16-byte header + concatenated asset bytes) + `index.bin` (`Vec<AssetMetadata>` with each variant rewritten to `AssetSource::Packed { offset, size }`).
2. **Stamp runtime** — the editor copies the pre-built `khora-runtime` for the chosen target into the output directory and renames it after the project. The runtime binary lives next to the editor (release-archive layout) or in the hub's `~/.khora/engines/<version>/runtime/` cache.
3. **Write `runtime.json`** — read by the runtime at boot to learn which scene to auto-load.

Cross-platform is trivial: the pre-built runtime exists for every target shipped by `release.yml`, so building Linux from a Windows host is just a file copy.

### Strategy B — Cargo build (native-Rust projects)

Used when the user has clicked **"Add Native Code"** on the project's hub card, which scaffolds `Cargo.toml` + `src/main.rs` (calling `khora_sdk::run_default()` by default — same as the runtime, but linked into a binary that *can* register custom components / agents / lanes when the user edits `main.rs`).

1. Same `PackBuilder` step as Strategy A.
2. `cargo build --release --manifest-path <project>/Cargo.toml` compiles the user's binary. stdout / stderr stream into the editor's console panel.
3. The compiled binary is copied into the output directory under the project name.
4. Same `runtime.json` companion is written.

Host-only in v1 (Rust cross-compilation is fragile without per-target toolchains). The editor refuses non-host targets on this strategy with a clear error pointing the user to "run the editor on the target OS".

### Output layout (identical between strategies)

```
<project>/dist/<target>/
├── <project_name>{.exe}     # renamed khora-runtime OR compiled user binary
├── data.pack
├── index.bin
└── runtime.json
```

Run the binary directly — both `khora-runtime` and any project compiled via `khora_sdk::run_default()` auto-detect PackLoader (when `data.pack` + `index.bin` are siblings) or FileLoader (when a loose `assets/` directory is a sibling instead, used by engine contributors who want to skip packing during iteration).

### Why two strategies

The choice is **deterministic** (presence of `Cargo.toml` is the contract) and **opt-in** (the user explicitly upgrades a project to native Rust by clicking the hub button). Most projects stay on Strategy A and benefit from trivial cross-platform export. Strategy B is the escape hatch when raw performance, compile-time safety, or new engine primitives are needed. Ergon scripts plug into both strategies transparently — they are assets under `assets/`, packed with the rest (see [Scripting](../concepts/scripting.md)).

## 10 — The Control Plane

The second Spine mode is the **Control Plane** — a workspace dedicated to the engine's mind.

| Region | Purpose |
|---|---|
| **Lane Timeline** (top) | Horizontal lanes per subsystem, execution windows as colored bands |
| **GORNA Stream** (bottom-left) | Live feed of negotiation: timestamp, subsystem, suggestion, accept/reject |
| **Meters Wall** (bottom-right) | Frame time, GPU %, memory, agent budget, assets pending |

The Control Plane is not a profiler popup. It is a first-class workspace. The whole pitch of Khora is the self-optimizing architecture; the editor surfaces that intelligence as a place you go to, not a window you launch.

Full design in [Editor design system](../design/editor.md), section 09.

---

## For game developers

You typically run the editor against your project folder:

```bash
cargo run -p khora-editor -- --project /path/to/my_project
```

Inside the editor, you author scenes. Each scene is one `.kscene` file. Spawn entities by dragging a vessel from the Assets panel into the viewport, or by right-clicking in the hierarchy → Add Entity. Edit components in the Inspector. Save with `Ctrl+S`.

To preview your scene in play mode, press the Play button. Your `EngineApp::update` runs every frame, exactly as in your shipping game.

## For engine contributors

The editor is an application built on the SDK — it implements `EngineApp` and
contributes its agents through `AgentProvider`.

| Where | Contents |
|---|---|
| `crates/khora-editor/src/panels/` | Scene tree, properties, asset browser, viewport, console, control plane |
| `crates/khora-editor/src/chrome/` | Title bar, menus, the window frame |
| `crates/khora-editor/src/widgets/` | Editor-specific widgets (tiles, inspector fields) |
| `crates/khora-editor/src/ops/` | High-level scene operations (spawn, despawn, parent, add component) |
| `crates/khora-editor/src/commands/` | The command dispatch the menus and palette go through |
| `crates/khora-editor/src/scene_io/mod.rs` | Scene save / load via `SerializationService` |
| `crates/khora-editor/src/gizmo/`, `picking.rs` | Gizmo geometry, drag math and dispatch; viewport picking |
| `crates/khora-editor/src/project_vfs.rs` | The live asset index — VFS, watcher and identity registry |
| `crates/khora-editor/src/project_scripts/` | Mounting the open project's scripts, and unmounting them |
| `crates/khora-editor/src/panels/problems/` | The Problems tab; `view.rs` is its pure view model and the `.erg` tile badge |
| `crates/khora-editor/src/ops/declared.rs` | Declared components as the Inspector reads and writes them — cards, edits, the add menu, prefab rows |
| `crates/khora-editor/src/widgets/inspector/declared/` | The declared cards and their typed field widgets |

`EditorState` — selection, mode, pending operations — is **not** in the editor
crate. It lives in `crates/khora-core/src/ui/editor/state.rs`, with the other
editor contracts (`EditorShell`, `EditorPanel`, `UiBuilder`), because the render
lanes and the egui backend need the same vocabulary and neither may depend on the
editor. The dock tree lives in `khora-tool-ui`; the gizmo interaction and the
command history live in the editor crate. The engine's design keeps that vocabulary backend-agnostic; the editor is
one consumer of it.

To add a panel: write its render function under `panels/`, register it in the
workbench, and route any mutation through `ops/` so the action layer stays the
single place the world changes.

To add a gizmo type: extend the interaction model in
`crates/khora-editor/src/gizmo/interact.rs` and its geometry in `gizmo/selection.rs`, draw it from the engine-side
`GizmoLane`, and convert drags into `Transform` mutations through `ops/`.

The editor depends on `khora-sdk` and `khora-tool-ui`, and nothing else from the workspace — there is no SDK bypass. Earlier revisions of this page described one; the manifest never had it.

## Decisions

### We said yes to
- **Editor as a separate binary.** The editor is a different application from a shipping game — different lifecycle, different active agents, different input.
- **Mode-first layouts.** Opinionated defaults beat infinite customization for 95% of users.
- **Play mode through scene snapshot.** Press Play, run the game; press Stop, you are back where you were. No state pollution.
- **The editor is an ordinary SDK application.** It depends on `khora-sdk` plus the first-party `khora-tool-ui` design system, which the SDK deliberately does not re-export so a game never compiles the engine vendor's brand.

### We said no to
- **Free-form panel docking.** Powerful but exhausting. We pick layouts users won't want to change.
- **Telemetry charts in the main UI.** Telemetry belongs in the Control Plane mode. Scene mode shows the scene.
- **Editor chrome during play mode.** Play mode is a near-shipping preview. Chrome reappears on stop.

## Open questions

1. **Multi-window.** How does Spine + Modes work when the user pops the viewport to a second monitor? Likely the popped window keeps its own Spine, with one mode lit.
2. **Plugin UI surface.** Third-party plugins need a place to live — probably the Inspector as additional component cards, but the contract is undefined.
3. **Collaboration.** Real-time multi-user editing (shared cursors, shared selections) is on no roadmap, but the architecture does not preclude it.

---

*Next: writing your own agent or lane. See [Extending Khora](../tutorials/extending-the-engine.md).*
