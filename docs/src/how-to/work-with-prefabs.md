# Work with prefabs

Make a prefab from part of a scene, place it, change one instance, and decide
which changes belong to that instance and which to every instance. For what a
prefab *is* — links, overrides, derived ids — see
[Concepts: prefabs](../concepts/prefabs.md).

---

## Make a prefab

Two ways, both in the editor, both writing a `.kprefab` file — a compact scene
file holding the entity and everything under it:

- **Save as Prefab…** — right-click the entity in the Scene Tree. A file dialog
  asks where to write it.
- **Drag the entity onto a folder** of the Asset Browser. The prefab is written
  there, named after the entity.

The entity you made it from stays as it was: making a prefab does not turn it
into an instance. Delete it and place the prefab if you want the scene to link
to the file.

## Place an instance

- **Double-click** the prefab in the Asset Browser — the instance appears at
  the scene's root.
- **Drag it into the viewport** — same, at the root.
- **Drag it onto an entity in the Scene Tree** — the instance becomes that
  entity's child.

Each placement is a new instance: a fresh root id, and members whose ids are
derived from it. **Duplicating** an instance gives another instance of the same
prefab, not a copy of its entities.

## Override a value

Select the instance — or any entity inside it — and change a field in the
Inspector as you would anywhere else. The field gains a dot, and its card a
note: that value is now the instance's own.

<div class="kp-figure-frame">

{{#include ../images/editor/inspector-prefab.svg}}

</div>

Moving, rotating or reparenting the instance's root is never an override: where
an instance stands is always its own. Scaling it is.

You can also add a component to an instance, or remove one the prefab has. An
added component's card says **added**; a removed one shows as a **removed**
row.

## Take a change back: revert

| To undo… | Do |
|---|---|
| one field | click the arrow at the end of its row, or right-click → **Revert to prefab** |
| one component | hover its card header → the revert icon |
| a removed component | **Restore** on its removed row |
| everything on this instance | **Revert all** in the instance band |

Reverting takes the prefab's value as it is now.

## Share a change: apply

| To push… into the prefab | Do |
|---|---|
| one field | right-click it → **Apply to prefab** |
| one component | hover its card header → the apply icon |
| every override of this instance | **Apply all** in the instance band |

Apply writes the prefab file. Every other instance of that prefab open in the
editor moves onto the new prefab at once — keeping its own overrides — and
every scene that links to it gets the change on its next load.

Apply refuses what cannot belong to a prefab: a value naming an entity outside
it (a parent that is not one of its members, a reference to something else in
the scene). It never writes the instance's own placement.

## Find the prefab

The instance band names the prefab — "Instance of `guard.kprefab`", or "Part of
an instance of" on a member. Click the name to select the prefab in the Asset
Browser.

## Load scenes that link to prefabs from code

A scene saved with instances holds links, so whatever loads it must be able to
read the prefabs. A shipped game does this already: the engine gives its world
a prefab source backed by the asset service. If you build a
`SerializationService` yourself, give it one:

```rust,ignore
use khora_io::serialization::AssetPrefabs; // the engine's source, over the asset service
use khora_sdk::SerializationService;

let service = SerializationService::with_prefabs(Arc::new(AssetPrefabs::new(assets)));
let report = service.replace_world(&scene_file, world)?;
```

Any type implementing `PrefabSource` will do — the editor's reads the project's
files.

A service made with `SerializationService::new()` reads no prefabs: it refuses
a scene that links to one, and leaves the world untouched.

Placing a prefab from game code while the game runs has no SDK call yet; the
editor is where instances are made today.

## When something goes wrong

- **"the prefab … cannot be read"** on load — the file a link names is missing
  or broken. Nothing was loaded. Restore the file, or open the scene in a build
  of the project that still has it.
- **"the prefab … contains itself"** — a prefab cannot contain an instance of
  itself, at any depth. The editor never writes one; a file that does is
  refused.
- **An instance saved as plain entities** — its prefab could not be read when
  the scene was saved. It is linked again the next time you save with the
  prefab readable.

## See also

- [Concepts: prefabs](../concepts/prefabs.md)
- [Scenes and game saves](../concepts/saves.md)
- [Editor reference](../reference/editor.md#prefabs)
