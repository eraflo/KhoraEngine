# Knowing what changed

<p class="kp-lede">A game world holds thousands of things, and most of them do not change from one frame to the next. Picture a warehouse where every box carries a sticker with the moment it was last touched: someone coming back after a while asks "what changed since I last looked?" and reads the stickers, without opening a single box. Khora's data layer keeps such stickers, so that whatever reads the world — the renderer, a system re-shaping text, a cache — does work only for what really changed.</p>

## Why it matters

Every frame, parts of the engine turn the world into something else: the render
flow turns transforms, meshes and lights into a list of things to draw, a UI
system turns text into shaped glyphs, a physics sync pushes moved bodies into the
solver. Done blindly, each of them redoes all of its work every frame, though most
frames change almost nothing. Knowing what changed lets each of them do only the
part that is new — and, when nothing is, nothing at all.

The answer must never be wrong in the dangerous direction. Being told about a row
that did not change costs a little work; *not* being told about one that did
leaves a stale picture on screen. So every mechanism on this page is built to err
only one way: it may report a little more than what changed, never less.

## The clock

The world keeps a clock that counts **ticks**. A tick is not a unit of time but
of work: the engine gives one to each run of a `DataSystem`, each run of a `Flow`,
and each wave of the scheduler — about 41 a frame in the sandbox scene. Whatever a
run writes carries that run's tick; whatever is written between runs (the game's
own update, an edit in the editor) carries the next one.

Whoever watches for changes — an **observer**, a `DataSystem` or a `Flow` — has a
last-seen tick: the tick of its previous run. What changed for it is exactly what
was written after that tick.

## Stickers on the rows

Components live in [pages](./ecs.md), one column per component. Beside each
column a page keeps:

- a **summary**, σ — the tick of the latest write to any of its rows;
- for a component someone watches, one **row tick**, τ, per row — the sticker;
- a **floor**, φ — "every row of this column was written at this tick or before",
  which stands in for the stickers when there are none.

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../images/ecs/change-floor.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>Stickers.</strong> Every write carries the clock's tick. Each row of a watched column keeps the tick of its last write; the column's summary keeps the latest of them.</li>
<li><strong>A write.</strong> At tick 9, <code>e3</code> takes damage. Its sticker becomes 9 and the summary rises to 9; no other row is touched.</li>
<li><strong>A reader asks.</strong> The renderer last looked at tick 7. The summary, 9, is past 7, so something changed; the stickers past 7 say which: <code>e3</code> alone.</li>
<li><strong>Quiet.</strong> Nothing is written for a long while and every reader has seen tick 9. The stickers are dropped, and a floor at 9 stands for all of them.</li>
<li><strong>A late reader.</strong> One that last looked at tick 4, under the floor, cannot be told which rows changed after 4 — so it is handed every row. More than needed, never too few.</li>
<li><strong>A new write.</strong> At tick 30, <code>e1</code> heals. The stickers come back: <code>e1</code> at 30, the others at 0, under the floor that covers them. Since tick 9, <code>e1</code> alone changed.</li>
</ol>
</figure>

The summary lets a reader skip a whole page in one comparison: a page whose
summary is not past the reader's tick holds nothing new, and none of its rows is
even looked at. Row ticks are kept only for components an observer watches, and
only while they move, so the memory they cost follows what is actually in use.

Moving a row is not a change. When an entity gains or loses a component, its row
moves to another page; when a page is compacted, rows change place. The row
carries its sticker with it, so a reader is not told about a value that did not
change. What moved is recorded apart, in the page's **order version**, ω, for
readers that care about the order of rows.

## Writing: the guard

Code that writes a component reaches it through a **`Mut<T>`** — what
`query_mut`, `get_mut` and `get_many_mut` hand out:

```rust
for (mut health, hit) in world.query_mut::<(&mut Health, &Hit)>() {
    health.points -= hit.damage; // the first write stamps this row
}
```

Reading through a `Mut` is not a change. The first write through it stamps the
row with the run's tick; later writes through the same guard cost nothing more.
Three ways around the plain write:

- `set_if_neq(value)` writes, and stamps, only a value that differs from the one
  held — the right tool for a system that recomputes a value every frame, like
  `transform_propagation`: an unmoved scene then writes nothing.
- `bypass_change_detection()` writes without stamping: the change is invisible to
  every reader. For bookkeeping no one should react to.
- `into_inner()` hands on a plain `&mut T` for the rest of the borrow, and stamps
  the row then, since what is done with it can no longer be seen.

The writes that do not go through a guard — `set_component`, `write_column`, a
script's `RowMut` field writes, a relayout of a script component — stamp the rows
they wrote, and only those: a value refused is not a write.

## Reading: `Changed<T>`

A query asks for changed rows with the **`Changed<T>`** filter and the tick to
compare against:

```rust
for (entity, text, _) in world
    .query::<(EntityId, &UiText, Changed<UiText>)>()
    .since(last_seen)
{
    // only the texts written after `last_seen`
}
```

`Changed<T>` reads only the stickers, never the value, so it may stand beside a
`&mut T` of the same component. Without `.since`, it compares against tick 0 and
every row counts as changed: an observer that has seen nothing sees everything.

A `DataSystem` is handed its own last-seen tick. Its registration names the
components it watches, and its function receives a `SystemContext`:

```rust
fn ui_text_layout(world: &mut World, runtime: &Runtime, _deck: &mut OutputDeck, ctx: SystemContext) {
    for (entity, text, _) in world
        .query::<(EntityId, &UiText, Changed<UiText>)>()
        .since(ctx.since)
    {
        // re-shape this text only
    }
}

inventory::submit! {
    DataSystemRegistration {
        name: "ui_text_layout",
        phase: TickPhase::PreExtract,
        run: ui_text_layout,
        order_hint: 0,
        runs_after: &[],
        observes: &[ComponentKey::of::<UiText>],
    }
}
```

`observes` is what makes `UiText` watched: its columns keep row ticks, so the
answer is row by row rather than page by page. A component is watched because an
observer declares it — never discovered by polling.

## Forgetting quietly

Row ticks cost eight bytes per row. A column that has been quiet for a while —
`QUIET_ROW_TICKS`, 12 300 ticks, about five seconds of the sandbox at 60 frames a
second — and whose last write every observer has already seen, drops them:
`ecs_maintenance` does it every frame. The floor rises to the summary, and the
next write brings the stickers back, zeroed under the floor. A reader that had
seen everything loses nothing; a late one is handed the whole page, as in step 5
above.

## A whole domain at once

Some readers want a coarser answer: has *anything* of a domain changed? Flows
caching their views ask this every frame. Each [semantic domain](./ecs.md) has a
**change epoch** — the tick of the latest write to one of its values, or the
latest move of a row of one of its pages. It is read in constant time, and two
equal epochs guarantee the domain is unchanged, values and order alike. Handing
out writable access does not move it; only a write, or a row changing place,
does. A flow folds the epochs of the domains it reads into its cache key, and on
a still frame republishes its previous view instead of projecting it again (see
[AGDF](./agdf.md)).

## What it guarantees, and its limits

- **No change is ever missed.** Every row written after a reader's tick is in its
  answer — whatever moved, compacted or dropped its stickers in between. The proof
  is in [the formal side](./change-detection-formal.md).
- **Exact when the stickers are there.** A reader whose tick is at or above the
  floor, on a column keeping row ticks, is told exactly the rows that changed.
- **Too much, never too little, otherwise.** Under the floor, or on a column no
  one watches, a written page is reported whole.
- **Two writes in one tick count once.** A tick is a unit of work, and no reader
  can look between two writes of the same run.
- **Ticks are never saved.** A scene or a save holds what the world is, not when
  it changed; a loaded world starts with a fresh clock, and its observers see
  everything once.
- **A query across domains does not skip pages.** When a query's terms live in
  several domains, a page may hold a stale column of another domain, so each row
  is judged through the entity's live location instead.

## Next steps

- [Change detection, formally](./change-detection-formal.md) — notation,
  invariants, the theorem and its proof, the bounds.
- [Data and the ECS](./ecs.md) — pages, domains and queries.
- [AGDF](./agdf.md) — the flow caches the domain epochs serve.
