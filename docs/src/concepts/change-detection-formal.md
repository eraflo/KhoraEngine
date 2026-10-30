# Under the hood: change detection, formally

<p class="kp-lede">This page is for contributors and the curious: the precise model behind <a href="./change-detection.md">knowing what changed</a>, what it guarantees, and why. Read the plain page first — this one is the same mechanism, stated so it can be checked.</p>

## Notation

- **The clock.** `t ∈ ℕ`, the world's current tick, starting at 1; `0` means
  "never". A run (a `DataSystem`, a `Flow`, a scheduler wave) begins at `t` and
  writes at `t`; ending it sets `t ← t + 1` (`World::begin_run`, `end_run`).
- **Observers.** For each observer `o`, `s_o`, the tick of its last run on this
  world (`World::observer_last_seen`; `0` if it never ran).
- **Columns and rows.** `c` ranges over page columns (one component in one page),
  `r` over its rows. `last(c, r)` is the tick of the latest write of row `r`'s
  value — carried along when the row moves.
- **Per column** (`ColumnTicks`): the summary `σ(c)`, the floor `φ(c)`, and,
  when present, the row ticks `τ(c, r)`. An absent array reads `τ = 0`.
- **Per page** `p`: the order version `ω(p)`, moved whenever a row of `p` changes
  place.
- **Per domain** `d`: the change epoch `E(d)`.

## Invariants

- **I0** — `s_o < t` for every observer `o`. The clock is always past what anyone
  has seen, so a write made now, inside a run or between runs, carries a tick no
  observer has seen.
- **I1** — `last(c, r) ≤ σ(c)` for every row `r` of `c`.
- **I2** — `last(c, r) ≤ max(τ(c, r), φ(c))`.
- **I3** — `φ(c) ≤ σ(c)`.

Each operation preserves them:

| Operation | What it does | Why the invariants hold |
|---|---|---|
| write row `r` at `t` (`Mut`, `set_component`, `write_column`, `RowMut`) | `τ(c,r) ← t` if the array exists, else `φ ← max(φ, t)`; `σ ← max(σ, t)` | `last(c,r) = t` is covered by `τ` or `φ`, and by `σ` |
| push a row (spawn, attach, migration) | its tick is the carried `max(τ, φ)` of the source, or `t` for a new value; `σ` rises to it | the same, for the new row |
| move a row (migration, swap-remove, compaction) | carries `max(τ, φ)`; `ω` moves | the moved row's bound travels with it; no value changed |
| materialise (first write to a watched column without row ticks) | `τ ← 0` for every row | `φ` still covers every existing row (I2 with `τ = 0`) |
| free (`free_quiet_row_ticks`) | drop `τ`; `φ ← max(φ, σ) = σ` | by I1, every `last ≤ σ = φ` |
| end a run | `t ← t + 1` | I0: the run's observer now has `s_o = t − 1 < t` |

## The answer a reader gets

An observer reading column `c` since `s` is handed

```text
Changed_s(c) =  ∅                          if σ(c) ≤ s
             =  { r | τ(c, r) > s }        if τ exists and s ≥ φ(c)
             =  all rows of c              otherwise
```

(`ColumnTicks::changed_since`). The rows it must see are
`A_s(c) = { r | last(c, r) > s }`.

**Theorem (no missed change).** `A_s(c) ⊆ Changed_s(c)`.

*Proof.* Take `r` with `last(c, r) > s`.
By I1, `σ(c) ≥ last(c, r) > s`, so the first case does not apply.
In the second case `s ≥ φ(c)`, and by I2 `max(τ(c, r), φ(c)) ≥ last(c, r) > s ≥ φ(c)`, so `τ(c, r) > s`: `r` is returned.
In the third case every row is returned. ∎

**Exactness.** If `τ` exists, `s ≥ φ(c)`, and every row with `τ(c, r) > s` has
`last(c, r) > s`, then `Changed_s(c) = A_s(c)`: the second case returns exactly the
rows with `τ > s`. A row's `τ` equals its `last` when it was written while the
array existed. Otherwise `τ` only over-states `last`:
- a row materialised under the floor reads `τ = 0`;
- a row carried out of a freed column reads `τ = φ_src ≥ last`.

The first is never returned once `s ≥ φ`. The second is returned only to a reader
with `s < φ_src`, a reader under the floor of the column the row came from.

**Readers stay above the floor.** A column is freed only when every observer that
ran has `s_o ≥ σ`. The floor becomes `σ`, so `s_o ≥ φ` holds for each of them, and
both sides only grow. The same holds for `φ_src` above. So every observer that
keeps running is answered exactly. Only one that never ran here, or one that
stopped running, reads under a floor, and it is answered safely. One that never
ran has `s = 0` and sees everything anyway.

**Same-tick writes.** Two writes of one run share its tick. No reader observes
between them: every reader runs at a tick of its own, and whatever is written
after its run carries at least `s_o + 1`.

## Domain epochs

`E(d)` is the latest tick at which a value of a component of `d` was written, or
a row of a page holding a column of `d` was pushed or changed place.

- It rises to `t` wherever `σ` rises (a stamp, a push) for a column of `d`, and
  wherever `ω` moves for a page holding one — every domain of a mixed page, not
  only the component's (`World::note_page_changed`).
- Handing out writable access moves nothing.
- It is a single `u64` per domain, read in O(1), and monotone.

**Contract.** If a reader computes `E(d)` at its tick `s` and again later, and the
two are equal, no value of `d` was written and no row of its pages moved in
between. *Proof:* any such change happens at a tick `≥ s + 1` (I0, and readers
run at their own tick), and raises `E(d)` to it, above the first reading, which
was at most `s`. ∎

## Bounds

- **The clock never wraps in practice.** At about 41 ticks a frame and 60 frames
  a second, a `u64` lasts some 2.4 × 10⁸ years.
- **Memory.** Each page column holds `σ`, `φ` and the slot of its row array:
  40 bytes, plus its map entry. A column keeping row ticks adds 8 bytes per row.
  In all, `40·#columns + 8·Σ |rows(c)|` over the columns that keep row ticks —
  watched components that moved in the last `QUIET_ROW_TICKS`.
- **Work.**
  - Stamping is O(1): a store and two `max`. A guard's first write also looks
    its column's ticks up in the page, once.
  - `Changed<T>` skips a clean page in one comparison. A dirty page costs one
    comparison per row, or none when the answer is the whole page.
  - Freeing walks every page column once per frame.
- **False positives.** They are bounded to the rows of written pages, for readers
  under the floor or on unwatched columns. A reader at or above the floor of a
  watched column has none.

## Where it lives

| Piece | File |
|---|---|
| `Tick`, `ColumnTicks`, `QUIET_ROW_TICKS` | `crates/khora-data/src/ecs/tick.rs` |
| the clock, stamps, freeing, domain epochs | `crates/khora-data/src/ecs/world/clock.rs`, `world/mod.rs` |
| `Mut<T>` | `crates/khora-data/src/ecs/mut_guard.rs` |
| `Changed<T>`, `FetchContext` | `crates/khora-data/src/ecs/query/changed.rs`, `query/fetch_context.rs` |
| `SystemContext` | `crates/khora-data/src/ecs/system.rs` |
| the runners that advance the clock | `crates/khora-control/src/substrate/{data_system_runner, flow_runner}.rs` |
| the property tests of the theorem | `crates/khora-data/src/ecs/tests/{no_change_missed, write_invariants, domain_epochs}.rs` |

## Prior art

- **Bevy** stamps components with ticks behind a `Mut<T>` guard, and filters
  with `Changed<T>` / `Added<T>` against a system's last run.
- **flecs** keeps per-table change counters, and so does Unity DOTS with
  per-chunk version numbers.

Khora takes the guard and the per-system tick from Bevy, and the per-page
summary from the chunk versions. It adds the floor, so that row ticks can be
dropped while no change is ever missed.
