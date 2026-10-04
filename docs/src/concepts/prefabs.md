# Prefabs

A **prefab** is a piece of a scene written down once and placed many times: a
guard with its sword and its shield, a torch with its light. Each placement is
an **instance**, and an instance stays **linked** to its prefab — edit the
prefab and every instance follows, except where an instance said otherwise.

This page explains what a link is, what an override is, and how Khora keeps
the two straight through saves and edits. For the clicks, see
[How-to: work with prefabs](../how-to/work-with-prefabs.md).

---

## A link, not a copy

A prefab is a `.kprefab` file: a scene file — compact encoding — holding one
subtree. Placing it in a scene does not paste that subtree in. The scene keeps
the instance **collapsed**: a link to the prefab, and the instance's
differences from it. Loading the scene **expands** each link from the prefab
*as it is now*; saving collapses it back.

<div class="kp-figure-frame">

{{#include ../images/persistence/prefab-link.svg}}

</div>

So a prefab edit reaches every instance on its next load — every field the
instance did not override — and a scene full of guards stays small: each guard
is a link and a handful of values.

## Identity inside an instance

Two instances of the same prefab hold the same entities — two swords, two
shields — and each needs its own identity: a save must be able to say *this*
guard's sword broke. An entity inside an instance is never given a stored id.
Its id is **derived**: `within(instance root, its id in the prefab)`.

<div class="kp-figure-frame">

{{#include ../images/persistence/persistent-id.svg}}

</div>

The same instance yields the same ids on every load, two instances never
share one, and nested instances compose: a lantern prefab inside the guard
prefab gives `within(within(guard, lantern), flame)`. A reference to an
instance's member — a script holding the guard's shield — survives the
prefab being edited, because the id depends only on where the member sits,
not on what the prefab looks like today.

## Overrides

An instance's differences from its prefab are its **overrides**. They are
recorded exactly the way a [game save](./saves.md) records its differences from
a scene — the same record, the same three-way merge — so the rules are the
same:

| The instance… | Recorded as | When the prefab changes |
|---|---|---|
| changed a field | the component, whole, beside the prefab's value then | the instance keeps its value; the prefab's other fields reach it |
| added a component | the component | it stays |
| removed a component | its name | it stays removed |
| deleted a member | the member's id | it stays deleted |
| reordered children | the order, where it differs | kept |

Two things are never overrides, because every instance has its own by nature:

- **where the instance stands** — its root's position, rotation and parent;
- **the link itself** — the `PrefabInstance` component that names the prefab.

Scaling the root is an override: it changes what the prefab looks like, not
where it is.

## Seeing and editing overrides

The inspector shows an instance against its prefab:

<div class="kp-figure-frame">

{{#include ../images/editor/inspector-prefab.svg}}

</div>

- the **instance band** names the prefab — "Instance of", or "Part of an
  instance of" on a member — and links to it;
- a **dot** marks every overriding field, and the arrow at the row's end puts
  the prefab's value back;
- a card notes how many fields it overrides, or that the instance **added** it;
- a component the prefab has and the instance removed shows as a **removed**
  row, with Restore.

Every override can go two ways. **Revert** throws it away and takes the
prefab's value. **Apply** writes it into the prefab file — one field, one
component, or the whole instance at once — and every other open instance of
that prefab is brought onto the new prefab immediately, keeping its own
overrides.

Apply never writes where an instance stands — the root's placement and every
member's parent belong to the instance — and refuses a value that would point
outside the prefab, such as a parent that is not one of its members.

## When a prefab cannot be read

A link is only as good as the file it names:

- **Loading** a scene whose prefab is missing, unreadable, or links back to
  itself fails — atomically, so the world is left as it was — and says which
  prefab. A half-expanded scene would be worse than none.
- **Saving** never fails for that reason: an instance whose prefab cannot be
  read whole is written expanded, as plain entities, and becomes a link again
  the next time it is saved with its prefab readable.

## Next steps

- [How-to: work with prefabs](../how-to/work-with-prefabs.md) — create,
  place, override, apply, revert.
- [Scenes and game saves](./saves.md) — the other difference Khora records,
  and the merge both use.
- [Serialization](./serialization.md) — records, persistent ids, encodings.
