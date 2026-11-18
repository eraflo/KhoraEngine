# Under the hood

<p class="kp-lede">For readers who want the reasons: why Khora has a language of its own rather than Lua or C#, how a frame's time becomes fuel, what the machine running a script looks like, which part of the engine owns what, and how a type or a function becomes usable from a script.</p>

## Why a language of its own

Every subsystem in Khora can be told *"you have 0.4 ms this frame, work with
that"*. The renderer drops to a cheaper lane, shadows lose resolution, the audio
mixer sheds voices. [GORNA](../gorna.md) exists so those answers are negotiated
rather than guessed.

Gameplay is the subsystem that cannot answer by itself. Hand a script a budget
and it either finishes or it does not, and if it does not, the choices are to
let it overrun the frame or to kill it mid-decision. Lua's debug hooks and
Wasmtime's fuel can *interrupt*, but interruption is an error path in both; a
budget needs stopping to be **ordinary**.

So an Ergon script that runs out of budget stops at the next **safepoint**, a
named place in its code where its state is complete, and keeps the machine that
stopped. The next frame resumes rather than restarts, and because every
safepoint has a name, the machine can be resumed in code the author edited
since.

## One frame of the script family

The time a script gets comes down the same path as every family's. The **DCC**
(the engine's budget keeper) lends the script family an **envelope**: time per
device, and memory. The family's agent turns it into a **budget**, the lane
turns the budget into **fuel** (a count of VM instructions), and the run reports
back how much of its work it got done.

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../../images/scripting/budget-frame.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>The envelope.</strong> At a frame start, the scheduler lends the DCC's envelope to the script family's <code>LaneAgent</code> (<code>apply</code>). The agent chooses the row that fits: <code>"whole"</code>, or <code>"critical"</code> when only the <code>[Critical]</code> behaviors fit.</li>
<li><strong>The terms.</strong> The scheduler wakes the agent, which sets its place's terms. The budget is the envelope's time on its busiest device, here 0.4 ms.</li>
<li><strong>The data.</strong> Before the lane runs, the <code>script_intake</code> DataSystem applies the reloads and joins the engine's events into the mail, and <code>ScriptFlow</code> publishes the <code>ScriptView</code> on the bus.</li>
<li><strong>The fuel.</strong> In the <code>TRANSFORM</code> phase the scheduler runs the layer installed at the place. <code>BudgetedScriptLane</code> reads the budget as a <code>#[budget]</code> input and turns it into fuel: the budget less the lane's measured overhead, times its measured rate.</li>
<li><strong>The turns.</strong> The VM walks every behavior in its share order and every instance of each. An instance whose share of fuel is gone sits out until next frame; a body cut short stops at a safepoint and resumes there.</li>
<li><strong>The report.</strong> The lane returns the share of its instances that ran (<code>done</code>) and whether one faulted. The place turns it into a report whose health is the lesser of <code>done</code> and how well the run kept to its budget, and the scheduler hands it to the DCC.</li>
</ol>
</figure>

## Fuel, priced and measured

Fuel is time turned into instructions, so the lane has to know how fast its
machine runs. It measures it, every run, on this computer.

- **The rate** is instructions per millisecond. It starts at 50,000
  (`INITIAL_RATE`). After each run the lane divides the fuel the turns spent by
  the time spent *inside* the turns, timed around each, and blends that into
  the rate at one tenth (`RATE_BLEND`, 0.1). A run too short to time leaves the
  rate as it was.
- **The overhead** is the rest of the run: delivering mail, settling shares,
  writing back. It is measured beside the rate, as the run's time less its
  turns' time, blended the same way. Timed together, the bookkeeping would read
  as slow instructions: a small fuel would read as a low rate, buy a smaller
  fuel next frame, and the rate would fall to nothing.
- **The fuel** of a budget is `(budget − overhead) × rate`. At the `"critical"`
  level it is cut to what the `[Critical]` behaviors need. Before the agent has
  lent a budget, the lane runs its level at its own price.

The price the agent quotes to the DCC goes the other way: the overhead, plus
the fuel the due turns need at the measured rate, plus 200 ns for each row the
systems' queries took last frame. A due instance is priced at its last
**whole** turn — never at a piece of one, or a cut budget would lower the price
that buys it back; failing that, at its behavior's other instances; failing
that, at 2 µs of fuel (`UNKNOWN_TURN`) or twice what its unfinished turn has
spent, at most 100,000 (`FUEL_PER_BEHAVIOR`, which also caps one turn's slice
and a goodbye). A `[Rate]` instance counts for the share of frames it runs; a
module not compiled costs nothing. The price is sure (confidence 1.0) once every
due turn is priced from a measured one, a guess (0.5) before.

## The machine

The VM is a **register machine**: a function's values live in numbered slots,
its **registers**, rather than on a stack of pushes and pops. One flat row of
registers serves the whole run; each call gets a window of it — its parameters
first, then its locals, then the temporaries of the expression being
evaluated, released as soon as it is done. A call places the callee's window
over the caller's arguments, so arguments are never copied. A register holds
a value whole, a `Vec3` or a `Quat` included; text, arrays and structs are
references into frame memory. The limits — 256 registers a function, 64 calls
deep, 65,536 objects of frame memory — are in
[Costs and limits](../../reference/ergon.md#costs-and-limits).

What a behavior instance keeps between turns is its **memory**, one per
instance, held by the `ScriptRuntime`: its fields, then which state it is in,
then the data of that state — one region, sized for the largest state, since
only one is live — then one countdown per timer. Text, arrays and structs are
kept there **by value**, never as references into frame memory, which is
emptied every frame. A body under way is kept beside it, with its machine.

## Who owns what

The split follows [CLAD](../clad.md): the agent chooses, the lane works, the
data layer keeps structure.

| Piece | Where | Why |
|---|---|---|
| The language and VM | `khora-script` | Depends on `khora-core` and the macros only, so the compiler and VM are tested without booting an engine |
| The scripting world (programs, live instances, pending mail, last report, measured rate and overhead) | `Runtime::services`, as `Arc<Mutex<ScriptRuntime>>` | It is subsystem state, not strategy state: the lane reads it as a `#[locked]` input, and the editor and the save code reach it there |
| Reloads, and the engine's events joined into the mail | the `script_intake` DataSystem (`khora-lanes/src/script/intake.rs`), before the lane | A program swap is upkeep between frames, not a lane's work |
| The work (delivery, running behaviors and systems, measuring) | `BudgetedScriptLane` | Lanes execute |
| A system's one entity | the `script_systems` DataSystem (`khora-data`) | Structure is the data layer's; a lane never spawns |
| Choosing a level for a budget | the script family's `LaneAgent` (`khora-agents/src/lane_agent/`), registered in `khora-lanes/src/script/registration.rs` | Agents are strategists and hold nothing else |

## Adding a type to the language

A value crosses the script boundary in three shapes: a VM register's `Value`, a
saved `Persisted`, and `khora-core`'s `ScriptValue`. The values that map one to
one across all three, `Bool`, `Int`, `Float`, `Entity`, `Asset` and the math
types, are listed once, in the `script_value_table!` X-macro
(`khora-core/src/script/table.rs`). Each crate generates its conversions
between the three shapes from that table, so the conversions cannot diverge.
The irregular variants (`Unit`, `Str`, `Array`, `Struct`, `Null`) are
hand-written arms. The macro generates nothing by itself: each crate calls it
with a macro of its own, which receives the rows (`Variant : RustType;`) and
writes that crate's conversions.

An engine math type becomes usable from a script in these places:

1. a variant in each of the three enums, of the same name: `Value`
   (`khora-script/src/vm/value.rs`), `ScriptValue` and `FrozenValue`
   (`khora-core/src/script/`);
2. a row in `script_value_table!`, which gives every conversion;
3. a line of the `ergon_type!` declaration in `khora-script::native::engine_types`,
   which gives the type its constructor and one accessor per component;
4. the arms the compiler's exhaustive matches then ask for, such as how the
   value is shown in an interpolation (`vm/display.rs`).

It is a **declaration and not an attribute**. An attribute belongs on the item
it describes, and these types live in `khora-core`, which carries no scripting
annotations. And a `ScriptType` needs a matching `Value` variant, so a game
cannot expose a type of its own this way whatever the syntax. What is written
down is not "this struct is scriptable" but "this table row is reachable from a
script". A conversion written by hand outside the table is the sign the table
is being bypassed.

## Adding an engine function

An engine function — `Abs`, `Log`, `Pressed` — is a Rust function marked
`#[ergon_fn]`:

```rust,ignore
use khora_script::ergon_fn;

#[ergon_fn(name = "Hypot", cost = 4)]
fn hypotenuse(a: f32, b: f32) -> f32 {
    (a * a + b * b).sqrt()
}
```

- `name` is what a script calls, by default the Rust name in PascalCase; `cost`
  is the fuel a call is charged, by default 1 — every engine function shipped
  today costs 1. A name already taken replaces the earlier function. The
  parameters' names, a leading `_` dropped, are the names a named
  argument uses.
- A function may take a `&mut NativeContext` first, to reach the world view and
  frame memory. It may not be `async` or take `self`. One returning a `Result`
  faults the turn with its error (`NativeFailed`).
- The attribute registers the function through `inventory`. The registry lists
  the language's built-ins first, then every registered function sorted by
  name, and compiled code calls a function by its place in that list.
- A call copies its arguments out, pushes no frame and runs whole: an engine
  function is never stopped part-way.
- **Text crossing.** A `String` parameter receives a copy of the script's text;
  a returned `String` is placed in frame memory, so it lives the frame, like
  any text a script builds. `String` is the only text type a function takes or
  returns.

The attribute is defined in `khora-macros` and used from
`khora_script::ergon_fn`; `khora_script` also re-exports `inventory`. What it
generates names `::khora_script::…`, so a crate adding functions — the
language's own built-ins included — needs `khora-script` alone. A game's crate
adds its functions the same way; `khora-sdk` does not re-export the attribute.

---

*Back to [Scripting](./index.md), or on to the [Ergon reference](../../reference/ergon.md), [Agents and Lanes](../agents-and-lanes.md) for the strategist/executor split, and [GORNA](../gorna.md) for how a frame's budget is decided.*
