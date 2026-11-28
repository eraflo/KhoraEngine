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
11. Restricted projects
12. For game developers
13. For engine contributors
14. Decisions
15. Open questions

---

## 01 — What the editor is

`khora-editor` is a separate binary built on the SDK. It opens a project (a folder containing `.kscene` files and assets), authors scenes through ECS-aware panels, and previews them with **play mode** — a one-button switch between editing and full simulation.

The editor is not a separate engine. It uses the same agents, lanes, and ECS as a shipping game. What Play changes is *which families run* — scripts, physics and audio run only while the game does — the *simulation clock*, and what the panels do on top of the world. The engine has no mode: Play and Stop are the editor's, and it tells the engine by switching families on and off.

The visual language — colors, typography, panels, voice — is documented in [Editor design system](../design/editor.md). This chapter covers the **architecture**, not the look.

## 02 — Workspace anatomy

The window has three fixed strips — the title bar, the spine and the status
bar — around one **dock** that holds every working panel. The Scene mode's
dock opens arranged like this:

```
+--------------------------------------------------------------+
| Title bar — brand, project name, menus, search               |
+----+---------+----------------------------------+------------+
|    |         |                                  |            |
| Sp | Hierar- |            Viewport              | Inspector  |
| in | chy     |                                  |            |
| e  |         +----------------------------------+            |
|    |         | Assets · Console · Problems      |            |
+----+---------+----------------------------------+------------+
| Status bar — health, branch, cpu, memory, fps, project       |
+--------------------------------------------------------------+
```

| Region | Purpose |
|---|---|
| **Title bar** | Fixed. Brand pill with the project name, the File, Edit, Build and Help menus, the search pill that opens the command palette. The window's own buttons are the system's |
| **Spine** | Fixed. Mode rail (Scene, Control Plane) |
| **Hierarchy** | Tree of entities — by default left of the viewport, the full height of the dock |
| **Viewport** | The 3D scene with floating gizmos |
| **Inspector** | Components of the selected entity — by default right of the viewport, the full height of the dock |
| **Assets · Console · Problems** | Three tabs of one group, under the viewport by default, Assets open |
| **Status bar** | Fixed. A health dot, the git branch, CPU, heap and video memory, a scene load in flight; on the right the frame rate and time, the project and engine version, and a *restricted* chip when the project is |

Everything inside the dock can be moved (see [Modes](#03--modes)); the
picture is only where it starts. The Control Plane mode's dock holds its one
workspace, full size. In a restricted project, a restricted bar sits under the
title bar ([Restricted projects](#11--restricted-projects)).

The visual language of these regions is in [Editor design system](../design/editor.md).

## 03 — Modes

Two workspaces ship today, switched from the Spine:

| Mode | Purpose |
|---|---|
| **Scene** | 3D editing (default) |
| **Control Plane** | Engine telemetry and GORNA stream |

Each mode has its own panel layout, held in a **dock**: drag a panel's tab onto
the middle of another group to join it as a tab, or onto one of its edges to
split it there. A splitter between two groups drags to resize them, never below
120 points a side. The layouts the editor starts with are only its defaults,
and a changed layout lasts the session: it is not saved, and there is no reset
yet. We still put the care into the defaults — most users keep them — and pick
layouts users won't want to change. Further authoring workspaces (2D canvas,
node graph, animation, shader graph) will be added as they are built.

## 04 — Play mode

<div class="kp-figure-frame">

{{#include ../images/editor/play-modes.svg}}

</div>

**Play waits for the scene.** While a scene is opening (one opened, a project's default scene) or Stop's restore is in flight, the world is not the scene the editor holds yet, and a snapshot of it would be restored at Stop as the wrong scene: Play is refused, and the log says why. Press it again once the scene landed.

When you press **Play**:

1. The current world is serialized using `SerializationGoal::FastestLoad` (a schema-bound snapshot), in memory.
2. The editor's `PlayMode` (UI state) becomes `Playing`.
3. The scripting, physics and audio families are switched on: scripts start, and every scripted entity arrives fresh and runs `OnSpawn`.
4. The simulation clock scale stays at `1.0`.
5. The play camera takes over from the editor camera.

**Pause** switches no family and sets the clock scale to `0.0`: scripts still take their turns, with a delta of zero, so nothing that counts time moves.

When you press **Stop** — from playing or paused:

1. The snapshot is restored with `replace_world`: atomic, every persistent id kept, every runtime `EntityId` fresh — the selection is cleared.
2. The editor's `PlayMode` becomes `Editing`; scripting, physics and audio are switched off, and the clock scale goes back to `1.0`.
3. The editor camera resumes.

Opening a project ends the play. Played from the scene the project let go, it ends without a restore: the snapshot is a scene of that project, so it is dropped, and a restore still in flight lands nothing. Played from a scene the user chose while the open was in flight, it ends as Stop ends it: that scene is restored as it was chosen ([Switching projects](#switching-projects)).

Stop is not a load of a game save: the next Play is a fresh start again, with `OnSpawn`, never `OnLoad`. A component whose schema the engine cannot trace whole is not guarded by the snapshot's fingerprint, so a world holding one is snapshotted as a compact record instead. See [Serialization](../concepts/serialization.md).

| Aspect | Editing | Playing | Paused |
|---|---|---|---|
| Families off | scripting, physics, audio | none | none |
| Scripts | do not run | run | run, delta `0` |
| Physics, audio | do not run | run | run; physics does not step at scale `0` |
| Other agents | all | all | all |
| Simulation clock | `1.0` — nothing that steps runs | `1.0` | `0.0` — no fixed sub-steps run |
| Camera | Editor camera (free orbit) | Scene cameras (active ones) | Scene cameras |
| Input | Editor input (gizmos, selection) | Game input (player controls) | Game input |
| ECS | Mutable — user edits directly | Snapshot taken — original world preserved | as Playing |

> **Two things cross from the editor into the engine, both set when the play
> mode changes** (`process_transport` in `khora-editor/src/commands/transport.rs`,
> run in `before_frame`): the clock's **scale** and the **families off**.
> Whatever changed the play mode — the transport, New Scene, a project opened
> while playing — they follow it; a family scoped in between, as a sound
> preview does, keeps its scope. `PlayMode` itself — buttons, panel visibility —
> stays in the editor.
>
> The scale makes the editor one caller among several: at `0.0` the fixed-step
> accumulator never fills, so no body integrates and no script timer counts
> down, while rendering carries on at real time — a pause menu, a cutscene and a
> slow-motion hit write the same number.
>
> The families are switched through the `Arc<Activation>` resource. `setup`
> opens editing with scripting (`khora_core::script::SCRIPT`), physics
> (`khora_core::physics::PHYSICS`) and audio (`khora_core::audio::AUDIO`) off.
> The switch lands at the frame boundary, before the first DataSystem phase, so
> a frame is wholly edited or wholly played. A family off is not lent a budget
> and does not run; the DataSystems and Flows that serve it
> (`Role::Serves(family)`) are skipped, and mail sent to it while it is off is
> dropped. A shipped game keeps every family on. The Control Plane summary shows
> the families off.

> **Physics state is not preserved across play mode.** Velocities, contacts, and sleep state are reset on restore. The ECS components are restored exactly; the physics world rebuilds from those components.

## 05 — Scene I/O

The editor uses `SerializationService` for all scene operations. As of v0.4 it goes through `ProjectVfs` (the editor's wrapper around `khora_io::AssetService` + `AssetWatcher`), so saves and loads inside the project use the same UUID identity that a release pack would:

| Action | What happens |
|---|---|
| **Open project** | `ProjectVfs::open` recursively scans `<project>/assets/`, builds the in-memory UUID index, registers all decoders, arms a filesystem watcher. The asset browser reads off the resulting VFS. |
| **New scene** | Editor creates an empty world, ready for editing. A scene still opening or being restored is withdrawn, and the new scene has no file yet: `Ctrl+S` asks for one. |
| **Save scene** (`Ctrl+S`) | Encoded with `SerializationGoal::EditorInterchange` (compact, records by name), prefab instances written as [links](#prefabs), and written via `AssetWriter` so the new file enters the VFS index immediately. The scene's file is named inside its project (`SceneInProject`: the project's `assets/` and the file's key under it), never by a bare path: a save of a scene of another project than the one open is refused and writes nothing. A scene with no file — a new one, one opened from outside the project, or any once its project is let go — is saved as **Save scene as** does. |
| **Save scene as** | Same, with a new path. The file picked is a scene file, by its extension in any case (`.kscene`, as the kind registry names a scene's), so the scene's file is only ever a scene: a name with no extension is saved as the `.kscene` of that name, as a file dialog adds it (`levels/forest` saves `levels/forest.kscene`), and a name with another kind's extension or an unknown one is refused, the log saying why. A file of another kind is never written over: **Save scene as** onto `textures/wall.png` writes nothing. A path inside the project's `assets/` — however it names the project's folder or a folder in it: through a link, a relative `--project`, or in another case where file names ignore case — is saved through the project (`ProjectVfs::in_project`, the one test the editor makes of it), named as its folders and file are spelled on disk, so it gets the identity a save there gives and opens again as the scene the index lists. Whether it is inside is decided on the path resolved. A path under `assets/` the project cannot keep is **refused**, and nothing is written, neither in the project nor beside it: a new name that is no [portable name](formats.md#asset-identity-registry) (ending with a dot or a space, a device name such as `nul.kscene`, a `:`…), never trimmed to fit, or a link in `assets/` leading out of them. The log says why. A path **outside** the project's `assets/` is refused too: a scene is saved in its project, where the index, the identity registry and Build Game see it, and never elsewhere; with no project open, nothing is saved. The dialog starts in the project's `assets/`. The file picked becomes the scene's only once the save landed: a save refused or failed changes nothing, the scene keeps the file it had and the Problems tab what it listed. Opening a scene and saving a prefab as tell inside from outside the same way. |
| **Load scene** | Double-click a `.kscene` in the asset browser, or File → Open. Project-internal paths route through `AssetService::load_raw` (UUID-keyed); prefab links expand from the project's prefabs, and the file becomes the one a save writes to. A file outside the project is read through a guard, without prefabs, and has no file a save writes to. A file that cannot be loaded leaves the open scene as it was. |
| **Play / Stop** | World snapshot + restore via `SerializationService` with `FastestLoad` — a schema-bound snapshot, in memory. |

### Switching projects

Opening a project — another one, the same one again, or one whose open fails
and leaves the editor with none — lets go of the project the editor held, in
one step, as the open resolves (`leave_project`, `app/project_open.rs`):

- every scene load the former project filed and that has not landed (an opened
  scene) is withdrawn through the engine's `SceneLoadHub`: it lands nothing;
- the play under way ends, always, and the editor is back in Editing. A play
  of the former project's scene ends without a restore: its snapshot is
  dropped and a Stop's restore still in flight is withdrawn, so no Stop can
  restore a scene of another project. A play of a scene chosen since the open
  was filed (below) ends as Stop ends it;
- the world is emptied as **New scene** empties it: every entity, the
  selection and what else names an entity, what the last load adapted and its
  `Load` problems;
- the scene's file is forgotten, so `Ctrl+S` can never write into the former
  project;
- the requests and edits queued against it (an entity, an asset or a problem
  of the project), the asset browser's listing, selection, open folder and the
  file operations it has not followed yet, the
  trust confirmation, and the problems the editor filed of the project (`Save`,
  `Leftover`) go; the undo history too.

The new project's default scene then starts from an empty world: opened by
ticket, or created and saved when there is none, and either becomes the scene
`Ctrl+S` writes to. A scene the user chose while the open was in flight (the
loading film gave up waiting) is kept, with its `Load` problems: only what was
filed before the open is withdrawn. Played since it was chosen (the editor
records the scene choice a Play started on, `EditorState::played_choice`), it
is kept as it was chosen, not as the play left it: the play ends through Stop's
restore of its snapshot, filed with the `SceneLoadHub`, and a Stop's restore
already filed for it is kept and lands. A Play asked as the project lands
waits for the project's default scene, as any Play waits for a scene opening
([Play mode](#04--play-mode)).

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

Empty folders are shown, so **New Folder** produces a usable target immediately. **Renaming a folder** moves the identity of every asset inside it, nested folders included, in one save of the registry; a registry that cannot be saved moves the folder back. A rename in case only (`Tex` to `tex`) is a rename too, where file names ignore case; a folder renamed so keeps the identities of the files trashed from it, which a restore brings back under the new spelling. A name typed in a rename box, a new folder or a new script is held to the portable name rule, the same on every system so a project moves between them: no `..`, `\`, `:`, control character or `<>"|?*`, no empty part, no name ending with a dot or a space, no Windows device name (`CON`, `NUL`, `COM1`, `LPT1`…, with any extension). Such a name is refused before anything moves, never changed to fit, the log naming the part refused. A file that already has such a name, made on another system, is not held to it: it can be renamed to a portable name, moved or deleted (see [Formats — paths the editor accepts](./formats.md#asset-identity-registry)). An entry a crash left under a rename's temporary name is put back when the project opens, or listed in the Problems panel when its name is taken. **Delete goes to the OS recycle bin** (via the `trash` crate), so a mistaken delete is reversible from the system trash rather than lost: the file keeps its identity in the project's identity registry, so restored at its path it gets it back, and every reference to it resolves again. A new file made at that path is another asset, with an identity of its own (see [Formats — asset identity registry](./formats.md#asset-identity-registry)).

**What the editor names by path follows its file.** After a rename or a move in the browser — of the file, or of a folder holding it — the scene `Ctrl+S` writes to, a scene still opening, the asset inspected and its tile, the requests queued that name a path, and what waits for a reindex (a reveal, a scene to open) name the file by its new key, so a save writes the renamed file and never makes the old one again. After a delete, what named the file, or a file of the folder deleted, is forgotten: the open scene has no file, so `Ctrl+S` asks where, as **Save scene as** does, and never makes the trashed file again; a material saved and waiting for the index to be assigned to its entity is assigned to none. One place takes what each operation moved or trashed and rewrites them all (`follow_key_changes`, `commands/asset_ops.rs`). The browser's own keys follow through the same route: each change is added to `EditorState::key_changes`, which the browser takes as it draws. The folder shown follows a rename or a move of itself or of a folder above it, the tree opened down to it; trashed with a folder above it or by itself, it gives way to the nearest folder above it left (`KeyChange::follow_folder`). The folders open, a folder being renamed and an entry a delete asks about follow too. The Inspector's prefab view and the asset pickers read the index again once it moved.

**While the identity registry is unread** (refused for its size, or a merge left conflict markers in it), Rename, Move, Delete, Duplicate and New Script are refused, and so is a scene, material or prefab saved to a path where no file is: the log says why, and that the file is to be mended. A material or a prefab saved over a file that is there is refused too, as its id is written into the prefab or assigned to the entity, and the id a renamed file holds is unknown meanwhile. Saving a scene over a file that is there still works. See [Trust — files of the project root](../concepts/trust.md#files-of-the-project-root).

**Drag & drop.** Drag a tile onto a folder in the tree to **move** it (the move is mediated by the editor, so the identity registry freezes the UUID and references survive). Drag a tile into the **3D viewport** to instantiate it: a mesh spawns an entity at the drop point, a prefab places a linked instance, a scene loads, and a texture or material assigns to the selected entity.

A drag remembers which version of the asset list it started from — the **asset
epoch**, moved whenever the list changes: a project opened, a file added,
renamed, moved or deleted, a change seen on disk. If the list
changed during the drag, the drop does nothing (logged at debug level) rather
than land a different asset that now sits where the dragged one was. The stamp
keeps 8 bits of the epoch, so it would mistake a list changed 256 times during
one drag for the one it started from.

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

**Save as Prefab…** follows the rule of **Save scene as** ([Scene I/O](#05--scene-io)), through the same decision: a prefab is saved in its project, where the index and its instances find it, and only as a `.kprefab`. The dialog starts in the project's `assets/`; a name with no extension is saved as the `.kprefab` of that name; a path outside them, a path there the project refuses, a file of another kind or of none, or any with no project open writes nothing, and the log says why.

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
runs](../concepts/scripting/turns.md#scripts-run-only-while-the-game-runs)).

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
raises such as `Touched`. So does a name whose file no project makes, a
Windows device's (`aux` gives `Aux2.erg`): the file shown and the file written
are always one the project accepts. The file
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

Each source of problems has a scope — what its problems describe, and so when
they leave (`ProblemSource`, `khora-core/src/problems.rs`):

| Source | Scope | Leaves |
|---|---|---|
| Compile, Relayout, Registration, Placement | the scripts mounted | with each compile, scan or mount |
| Running, Data | the world as it is now | as soon as it is so no more |
| Load | the open scene | with it: a new scene, another load landing |
| Save, Leftover | the project open | as the next open resolves, whether it lands or fails |
| Startup, Security | the run | a file released or read again |
| Restricted | the trust of each root watched | listed again whenever a trust or a root moves |

So a project change keeps the `Load` problems of a scene the user chose while
the open was in flight, as it keeps the scene, and the Problems tab always
lists what the scene's own load reported. A failed open leaves the editor with
no project; why it failed joins the load problems of the scene it was left on.

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
| `Prefab`, `Sound`, `Material`, `Mesh`, `Texture`, `Scene`, `Font` | a slot naming the asset, its folder beside it — drop one of that kind from the Asset Browser, or click it to pick one; right-click → *Choose…*, *Reveal in Assets*; an asset gone from the project reads `missing · …` in amber |
| `T?` | the value, or `none` with **Set** (an asset or entity slot shows its own `none`); right-click → *Set to none* |
| `T[]` | a row per item, **+ Add item** (for assets, it opens the picker); right-click an item to move or remove it |
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

Pick the tool from the pill at the viewport's top left — Hand (select only),
Move, Rotate, Scale — or with `Q`, `W`, `E`, `R` (not while `Ctrl` is held).
The Hand draws no gizmo.

**How the handles behave.**

- Move and Rotate follow the **world's** axes; Scale follows the **object's own**
  axes, those of the first selected entity. With several selected, the gizmo
  stands at the average of their positions and moves them all.
- The gizmo keeps **one size on screen** however far the camera is: about a third
  of the viewport's half-height. A handle is grabbed within 14 % of that size of
  its arm or ring, and a handle under the cursor wins over the object behind it.
- A drag is measured **from where it started**, not added up frame by frame, so
  it never drifts: moving back to the start puts the entity back. A rotation
  counts whole turns past half a turn; a scale never goes below 1 % of where it
  started. A drag ends when the button is released, the tool changes or the
  window loses focus. Each entity dragged is marked `Teleported`, so physics
  takes the new place as given ([Physics](../concepts/physics.md)).
- A **child** is moved in its **parent's** frame: the drag's world-space change is
  written into the child's `Transform`, which is relative to its parent. Under a
  rotated or scaled parent, a child moves along the parent's axes, not the
  world's.

**Picking.** A left click in the viewport selects what is under the cursor —
on the click only, never on hover. A ray goes from the camera through the
cursor and meets each entity's **box**: a mesh's box from its vertices, and for
anything without a mesh (a light, a camera, a sound, an empty) a small cube,
0.35 units from its origin each way. The nearest box hit wins; a box the camera
stands inside cannot be picked. `Ctrl`+click adds or removes the entity from the
selection; a click on empty space clears it, unless `Ctrl` is held. Picking and
the gizmos work while playing too.

**The camera.** While editing, with the cursor in the viewport: drag with the
middle or the right button to orbit around the point the camera looks at;
`Shift` + middle-drag to pan; the wheel to zoom, from 0.5 to 500 units away.
There is no fly mode and no key to frame the selection yet. While playing, the
scene's own camera renders and the editor's does not move.

**No undo yet.** The Edit menu holds Delete only, and `Ctrl+Z` / `Ctrl+Y` are
left unbound: the editor has a command history (`CommandHistory`), but nothing
records an edit into it, so nothing could be undone
([#69](https://github.com/eraflo/KhoraEngine/issues/69)).

Selection is tracked in `EditorState.selection` (a set — multi-select is supported), with `EditorState.inspected` naming the one the Inspector shows. Clicking an entity in the hierarchy or the viewport sets it; `Esc` clears it. Selected entities get a 2 px gold inner-stroke outline (1 px dark outer stroke), visible against any background.

`EditorState.hidden_entities` dims the rows it names in the scene tree, but
nothing fills it: the editor has no way to hide an entity yet (no eye icon, no
model of an entity switched off), and picking, the gizmos and rendering ignore
it.

Numeric fields in the Inspector are draggable scrubbers — drag horizontally to change a value, modifier keys for precision. No spinner buttons.

## 09 — Build Game

`Build → Build Game…` packages the open project for a target OS. The editor picks one of two **strategies** automatically, based on the presence of `<project>/Cargo.toml`:

### Strategy A — Runtime stamp (default, data-only projects)

Used when the project has no `Cargo.toml` — the typical state for a fresh project from the hub.

1. **Pack** — `khora_io::asset::PackBuilder` walks `<project>/assets/` (sorted by forward-slash relative path), resolves each file's UUID through the project's identity registry (a frozen entry if one exists, else the `AssetUUID::new_v5(rel_path)` default — the same registry the editor's dev VFS reads, so dev and release UUIDs match by construction; see [File formats — asset identity registry](formats.md#asset-identity-registry)), and writes the release layout: `data.pack` (24-byte header + each asset's bytes, compressed or not) + `index.bin` (`Vec<AssetMetadata>` with each file's variant rewritten to `AssetSource::Packed`, a sub-asset keeping its `Sub`) + `manifest.bin`, signed with your key (see [Trust: signed packs](../concepts/trust.md#signed-packs)).
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

### Result

The build is made in a temporary folder beside `<project>/dist/<target>/`, and
swapped in whole once it succeeded: a failed build leaves the previous one as it
was, and its temporary folder removed.

A build killed on its way (the editor closed, the machine stopped) can leave
what it was doing beside the folder: its temporary folder
(`<target>.<pid>.<n>.tmp`), or the previous build it had moved aside to swap the
new one in (`<target>.<pid>.<n>.previous.tmp`), with no folder at `<target>`.
The next build reclaims them before it starts: with no folder at `<target>`, it
puts the previous build back; it removes every other one. Only a folder named by
that pattern after the target is touched. The result line says what was put
back or removed, and so does the error of a build that then fails; a folder
that cannot be removed (another program holds a file of it) is named there as
left, as is a previous build the swap could not remove.

One build of a target runs at a time, across processes. A build holds an OS
lock on `<project>/dist/<target>.lock`, beside the folder, from its start to its
end — the reclaim and the swap included — so it never reclaims the temporary
folder of another build still running. A second build of the target, in this
editor or another, is refused at once, and its result line says another build
holds the target: a build runs on the editor's frame, and waiting would freeze
the editor behind another's build. The lock file is left in place; the system
releases the lock of an editor that ends, killed or not.

The console's result line says `built` and where, with the asset count and the
bytes packed. An external file the pack left out — refused by the guard, over its
container's cap, or missing — is not packed, and the build goes on: the line then
says `built, N files left out` and names each file and why. An asset refused fails
the build, and the line says which and why. Every refusal also reaches the Problems
panel (see [Trust — the guard](../concepts/trust.md#the-guard)).

### Output layout (identical between strategies)

```
<project>/dist/<target>/
├── <project_name>{.exe}     # renamed khora-runtime OR compiled user binary
├── data.pack
├── index.bin
├── manifest.bin             # the pack's signed manifest
└── runtime.json
<project>/dist/<target>.lock # held while a build of the target runs
```

Run the binary directly. It opens the pack beside it (`data.pack` + `index.bin` + `manifest.bin`) once its seal checks against the key stamped in the binary (see [Trust: signed packs](../concepts/trust.md#signed-packs)). A stamped binary refuses loose files; an unstamped one, such as `cargo run` while iterating, reads a loose `assets/` folder beside it instead.

### Why two strategies

The choice is **deterministic** (presence of `Cargo.toml` is the contract) and **opt-in** (the user explicitly upgrades a project to native Rust by clicking the hub button). Most projects stay on Strategy A and benefit from trivial cross-platform export. Strategy B is the escape hatch when raw performance, compile-time safety, or new engine primitives are needed. Ergon scripts plug into both strategies transparently — they are assets under `assets/`, packed with the rest (see [Scripting](../concepts/scripting/index.md)).

## 10 — The Control Plane

The second Spine mode is the **Control Plane** — a workspace dedicated to the engine's mind.

| Region | Purpose |
|---|---|
| **Summary bar** (top) | The DCC at a glance: the families switched off, the budget multiplier, the agent count and the frame rate in two lines; then five cells — frame budget, CPU, GPU, video memory, heap. The frame budget cell draws the frame time over the last 48 frames as a sparkline, the only cell with a history: the others are instantaneous, drawn as bars. A sample is taken only while the Control Plane is shown, so the line has gaps for the time spent in another mode |
| **Agents** | Every registered agent, grouped by importance (Critical, Important, Optional), with its running row and health |
| **Schedule** | The agents by execution phase. A schedule, not a timeline: per-phase timing is not measured yet, and a custom phase is not shown |
| **Inspector** | The selected agent: its timing and status, and the last GORNA decision for it — the envelope lent, the row planned and the row running, what opened the round and what bound the frame, who decided |

With no registry — the engine not booted, a headless test — the workspace says
*no agents registered* instead of showing anything made up.

The Control Plane is not a profiler popup. It is a first-class workspace. The whole pitch of Khora is the self-optimizing architecture; the editor surfaces that intelligence as a place you go to, not a window you launch.

Full design in [Editor design system](../design/editor.md), section 09.

## 11 — Restricted projects

A project you did not make opens **restricted** until you trust it: you can read
and edit everything, but nothing of it runs, and its files are read under the
strict size caps. Why, and how files are checked, is in [Trust](../concepts/trust.md).

**Deciding.** Opening a project nothing was decided of asks first — in the hub
or in the editor: *Close project*, *Open restricted* or *Trust and open*. A
project created in the hub is trusted without asking. The hub marks a restricted
project's card with a **Restricted** chip, and its *More* menu offers *Trust
this project…* or *Restrict this project*.

**In a restricted project**, the editor shows a **restricted bar** under the top
bar — a lock, *Restricted mode*, and *Trust this project…*. What does not run is
refused with a message:

- **Play** is refused — the project's scripts would run. A project that turns
  restricted during Play is stopped.
- **Build Game** is refused — building compiles and runs the project's code.
- A **shader overlay** is ignored, and listed as a warning in the Problems panel.

**Trusting** — *Trust this project…* — first shows what it will let run, counted
from the project: its scripts under `assets/scripts`, its shader overlays under
`assets`, a `Cargo.toml` at its root, and the files its size caps refused.
*Trust and reopen* then reopens the project: every reader is rebuilt at the new
trust, the files refused for their size are released, and the Restricted
warnings are cleared. A file refused for what it holds, or downloaded from the
internet, stays refused.

**Quarantined files.** A refused file shows a lock on its tile in the asset
browser. Selected, the inspector's quarantine card says what refused it (the
file guard, a decoder, the type check, the pack), the trust it was read at, and
what it was kept by — its content's hash, or its size and date. **Release**
takes it out of the quarantine and loads it again; refused again, it goes back.

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
`PhaseProvider`, installs what it needs in the runtime through
`EngineApp::provide`, and registers no agent of its own.

| Where | Contents |
|---|---|
| `crates/khora-editor/src/panels/` | Scene tree, properties, asset browser, viewport, console, control plane |
| `crates/khora-editor/src/chrome/` | Title bar, menus, the window frame |
| `crates/khora-editor/src/widgets/` | Editor-specific widgets (tiles, inspector fields) |
| `crates/khora-editor/src/ops/` | High-level scene operations (spawn, despawn, parent, add component) |
| `crates/khora-editor/src/commands/` | The command dispatch the menus and palette go through |
| `crates/khora-editor/src/scene_io/mod.rs` | Scene save / load via `SerializationService` |
| `crates/khora-editor/src/gizmo/`, `picking.rs` | Gizmo geometry, drag math and dispatch; viewport picking |
| `crates/khora-editor/src/project_vfs/` | The live asset index — VFS, watcher and identity registry; `asset_files.rs`, the asset browser's file operations |
| `crates/khora-editor/src/project_scripts/` | Mounting the open project's scripts, and unmounting them |
| `crates/khora-editor/src/panels/problems/` | The Problems tab; `view.rs` is its pure view model, and re-exports the `.erg` tile badge of `widgets/tile_badge.rs` |
| `crates/khora-editor/src/ops/declared.rs` | Declared components as the Inspector reads and writes them — cards, edits, the add menu, prefab rows |
| `crates/khora-editor/src/widgets/inspector/declared/` | The declared cards and their typed field widgets |

`EditorState` — selection, mode, the queued `EditorRequest`s — is **not** in the editor
crate. It lives in `crates/khora-core/src/ui/editor/state.rs`, with the other
editor contracts (`EditorShell`, `EditorPanel`, `UiBuilder`), because the render
lanes and the egui backend need the same vocabulary and neither may depend on the
editor. The gizmo interaction and the command history live in the editor crate.
The engine's design keeps that vocabulary backend-agnostic; the editor is
one consumer of it.

**The dock is a panel.** The dock tree and its widgets (tab strips, splitters,
the drop overlay, the drag ghost) live in `khora-tool-ui`, the branded design
system. The shell that hosts panels, `EguiEditorShell`, lives in `khora-infra`,
which depends on `khora-core` alone and must never depend on `khora-tool-ui`.
So the dock is not part of the shell: it is one ordinary `EditorPanel`, the
`WorkbenchPanel` (`panels/workbench.rs`), registered at the centre. It holds one
`DockTree` per mode and the working panels by id, paints the dock widgets
through `UiBuilder`, and runs each group's active panel inside
`UiBuilder::region_at`. The shell only ever sees `Box<dyn EditorPanel>`.

**Widget ids and keyboard focus.** The egui backend keeps focus and text-edit
state by widget id, so ids must stay the same from frame to frame:

- `UiBuilder::interact_rect(salt, …)` derives its id from the salt **under its
  parent**; `region_at(salt, …)` names its region by the salt, never by its
  rect, so a field keeps its focus while a splitter moves. A salt must be stable
  across frames and unique within its parent.
- Each workbench panel's region is salted by its panel id, each tab strip by
  its group (`wb-g{n}`). What repeats per entity carries the entity: an
  Inspector card's controls are salted `card-hdr-{index}.{generation}::{title}`
  and the like, a scene-tree row's chevron `st-chev-{index}`; asset tiles by
  their place in the list (`tile-{i}`).
- The inline rename field's id is global (`khora_inline_edit` with its salt),
  not under its parent: one field is edited at a time.

A panel's own shortcut reads `UiBuilder::key_pressed`, which answers `false`
while a text field has keyboard focus, so a single-key binding never eats what
the user types; `keyboard_captured` says whether one has. `raw_key_pressed`
ignores focus and is for the widget that owns the field — the command palette
moving its selection while its query is typed. `focus_last_item` gives a field
focus: call it on the frame the field appears, never every frame, or the user
can never leave it. The editor's global shortcuts (`Q W E R`, `Delete`,
`Ctrl+K`, `Ctrl+S`, `Ctrl+1`/`2`, `Esc`) arrive as engine input events; a key
egui used is dropped before it becomes one. Pointer events inside the viewport
always reach the engine, whatever egui says; while the loading cover is up,
every event goes to egui alone.

To add a panel: write its render function under `panels/`, register it in the
workbench, and route any mutation through `ops/` so the action layer stays the
single place the world changes.

To add a gizmo type: extend the interaction model in
`crates/khora-editor/src/gizmo/interact.rs` and its geometry in `gizmo/selection.rs`, draw it from the engine-side
`GizmoLane`; a drag's result is written onto the dragged entities by
`gizmo::apply_delta` (`gizmo/frame.rs`).

The editor depends on `khora-sdk` and `khora-tool-ui`, and nothing else from the workspace — there is no SDK bypass. Earlier revisions of this page described one; the manifest never had it.

## Decisions

### We said yes to
- **Editor as a separate binary.** The editor is a different application from a shipping game — different lifecycle, different active agents, different input.
- **Mode-first layouts.** Each mode opens on a default chosen with care; most users never change it.
- **A dock under the defaults.** A tab drags onto another group, or onto one of its edges to split it, so a user who needs another arrangement is never stuck with ours.
- **Play mode through scene snapshot.** Press Play, run the game; press Stop, you are back where you were. No state pollution.
- **The editor is an ordinary SDK application.** It depends on `khora-sdk` plus the first-party `khora-tool-ui` design system, which the SDK deliberately does not re-export so a game never compiles the engine vendor's brand.

### We said no to
- **Telemetry charts in the main UI.** Telemetry belongs in the Control Plane mode. Scene mode shows the scene.
- **Editor chrome during play mode.** Play mode is a near-shipping preview. Chrome reappears on stop.

## Open questions

1. **Multi-window.** How does Spine + Modes work when the user pops the viewport to a second monitor? Likely the popped window keeps its own Spine, with one mode lit.
2. **Plugin UI surface.** Third-party plugins need a place to live — probably the Inspector as additional component cards, but the contract is undefined.
3. **Collaboration.** Real-time multi-user editing (shared cursors, shared selections) is on no roadmap, but the architecture does not preclude it.

---

*Next: writing your own agent or lane. See [Extending Khora](../tutorials/extending-the-engine.md).*
