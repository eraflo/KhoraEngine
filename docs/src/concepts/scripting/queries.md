# Queries and sweeps

<p class="kp-lede">A <strong>query</strong> is a question asked of every entity at once: <em>who has health, regenerates, and is not dead?</em> — like asking a room to raise a hand. A <strong>sweep</strong> then visits each raised hand, one at a time, and may change what it finds. The list of hands is taken once, when the sweep starts, so what the sweep changes along the way never confuses it.</p>

## Queries and sweeps

`foreach (…)` with no `in` is a **query**: its bindings name what every row
holds, and its body runs once per row — a **sweep**.

```text
foreach (Entity e, ref Health h, Regen r, Shield? s) with Player without Dead { … }
```

| Part | A row… |
|---|---|
| `Entity e` | binds the row's entity — anywhere in the list, once |
| `Regen r` | holds a `Regen`; `r` is a copy of it |
| `Shield? s` | may hold a `Shield`; `s` is a copy, or `null` — it never filters |
| `ref Health h` | holds a `Health`; `h` is a copy written back at the row's end — in a system only |
| `with Player` | holds a `Player`, not read |
| `without Dead` | holds no `Dead` |

**Outside a system, a query is read-only.** A behavior may sweep — a census, a
search for the nearest foe — but binds without `ref`, and writes another
entity's component with `e.Set(…)`, as any script does: writing rows in place is
what a system is for. A system in turn may read a behavior's component in a
query, never `ref` it: a behavior's fields are written by its own turn or by a
`Set`, and its turn builds on the `Set`s queued before it — a column write it
never read would break the frame's [one write order](./writing.md#a-script-never-writes-the-world).
Every component a query names, filters included, joins the program's reads, so
the frame projects it.

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../../images/scripting/sweep.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>The query.</strong> A system asks for every entity holding <code>Health</code> and <code>Regen</code>, and no <code>Dead</code>.</li>
<li><strong>The rows, taken once.</strong> The answer is a list, in entity order, taken when the sweep starts. Each row is checked again when it is reached: <code>e8</code> died since, and is skipped.</li>
<li><strong>A row's end.</strong> Where the body leaves a row, the fields it changed — here <code>current</code> — are written. The rest are left alone.</li>
<li><strong>Where the write goes.</strong> At once into the working set, so every later row and script reads it, and into a pending batch for <code>Health</code>.</li>
<li><strong>The run's end.</strong> However the run stops, each batch leaves as one column write: ten thousand rows, one command.</li>
</ol>
</figure>

### The rows are taken once

When the sweep starts, the query takes its rows: the entities holding every
component bound without `?` and every `with`, and none of the `without`, **in
entity order**, from the frame's working set — what the frame projected, with the
adds, removals and writes the frame's scripts have queued so far. The list is
held by the machine, so a structural change the body makes — `e.Remove(Regen)`,
`e.Add(Dead { })` — is a command, and leaves the sweep alone. Each row is
**checked again** when it is reached: one an earlier row's body took out of the
query is skipped. An entity spawned this frame is not among the rows — it exists
from the frame's end; a despawned one keeps its rows until then, as it keeps its
components. Taking the rows visits the holders of the rarest component the query
requires, so it costs what the query matches, not what the scene holds. A query
run again — inside a loop, or next frame — takes its rows again.

### `ref`: written where the row ends

A `ref` binding is a copy like the others, except that its fields may be
assigned — `h.current += 1.0` — and are written back; the binding itself is
never reassigned. Where the row ends — after the body's last statement, where a
`continue` lands, before a `break` or a `return` leaves the sweep — the fields
that **differ** from the row as it was read are written:

- **into the working set, at once** — a later row, a later sweep of the same
  run, and every script after it in the frame read the new value;
- **into the run's pending batch** for that component and that set of fields.

A batch leaves as **one column write** (`WriteColumn`) — early, before any later
`Set`, `Add` or `Remove` of that component in the run, and in any case when the
run stops, however it stops: finished, cut by fuel, faulted. No batch crosses a
frame or a save, and the run's writes reach the `World` in the order they were
made: ten thousand rows writing `current` are one command, and a run writing
`Health`, then `Regen`, then `Health` again leaves each field as it last wrote
it. A row left as it was read writes nothing; a field the row did not change
keeps whatever another script set this frame. Rows that change another set of
fields start a batch of their own. A fault takes back what the run queued this
frame, its rows with the rest ([below](./writing.md#a-faulted-turn-leaves-nothing)).

### A sweep short of fuel

A sweep stops like any body — at a safepoint: the head of the next row, or a
statement inside one, even part-way through a row. The rows it ended leave with
the run; next frame the machine resumes exactly where it stopped, on its row,
over the list it took, every later row checked against the working set of the
frame it is reached in. Each row is written once. The body keeps the `dt` it was
called with, the delta of the frame its pass began in. The list and the position
are locals of the frame, so a game save freezes a sweep part-way like any body,
and an edit [rebuilds](./saves-and-edits.md#a-body-part-way-through-in-tiers) it at its site.

GORNA prices a system by its rows: a system is one instance however many rows it
sweeps, so the script agent estimates 2 µs per instance plus 0.2 µs per row the
frame's queries took the frame before.

---

*Next: [Events](./events.md). Back to [Scripting](./index.md).*
