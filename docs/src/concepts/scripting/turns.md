# Turns and time

<p class="kp-lede">Every frame, each behavior gets a <strong>turn</strong>, like players around a board game: one after the other, always in the same order. A turn has an order of its own — first what happened to the entity, then what it was doing, then what it does now. And because a frame has a time limit, a turn can be stopped in the middle — at a clean place, like closing a book on a bookmark — and picked up exactly there next frame.</p>

## One turn

Each frame, each behavior gets a **turn**. A turn runs its steps in a fixed
order, each only when it applies:

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../../images/scripting/turn.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>Before the first turn.</strong> The behavior's fields are made from their defaults — plain data; no code runs.</li>
<li><strong>What happened to it.</strong> Restored from a save: <code>OnLoad</code>. New: <code>OnSpawn</code>, once ever. An edit lost what it was doing: <code>OnResumeFailed</code>.</li>
<li><strong>What it was doing.</strong> A body still under way — waiting on an <code>await</code>, or cut short last frame — picks up where it stopped. Still waiting: the turn ends here.</li>
<li><strong>What is due.</strong> The timers that are due, one <code>FixedUpdate</code> per tick of the fixed clock it is owed, then the events raised last frame.</li>
<li><strong>What it does now.</strong> <code>Update</code>, last, with the time since its last turn. Out of fuel at any step, the turn stops at a safepoint and goes on next frame.</li>
</ol>
</figure>

The order is the guarantee. `OnLoad` runs before anything can observe a restored
instance. A body still under way — waiting on an `await`, or cut short by the
budget — resumes before anything new starts, so a behavior never runs two
answers to "what am I doing" at once; and while it is still waiting, nothing
else of that behavior runs this frame.

### Turns in one order

The frame runs behaviors and systems in **one list**: by module path, then by
where each is written in its module — a system declared between two behaviors
takes its turn between theirs — and a behavior's instances in entity order. It
is the order a reader can check by reading the files; nothing reorders it yet.
It matters because a script reads what the scripts before it wrote this frame
([below](./writing.md#a-script-never-writes-the-world)).

## Stopping is ordinary

Fuel is charged per instruction but **checked only at safepoints**: the start of
a statement, the head of a loop, a function's entry, just past a call's return.
A run that has spent its fuel goes on to the next one and stops there; what it
spent past the budget is reported as an overdraft, never hidden. Every loop's
back edge lands on a safepoint, so the stretch between two of them is
straight-line code: the compiler measures the worst one, and a run never
overdraws more than that.

<div class="kp-figure-frame">

{{#include ../../images/scripting/sites.svg}}

</div>

Stopping only there is what keeps a stopped machine small and meaningful. At a
statement's start no half-evaluated expression is live — the machine is its
locals and a position. And every such position, and every place a frame stands
after an `await` or a call, is a **site** with a name the compiler derives from
the source: the kind of each enclosing statement and a short hash of what it
says. Those names are what let a machine survive an edit (below).

### Text outlives the stop

Text a body builds — `"hp " + name`, a name an engine function returned, an
event's argument — lives in frame memory, emptied every frame. A stopped body
keeps it anyway: on stopping, the machine copies the text its registers name
into itself, and when it resumes puts it back where running code reads it. A
field and a state's data keep text by value for the same reason, so `become
Chase("x" + name)` stores the text, not a reference to it. Frame memory is one
for the whole run and counts the frames, so text kept past its frame by any
other route fails to read — it never reads what a later frame put in its
place.

### A body that never finishes

Fuel bounds a frame, not a body. A loop with no `await` — `while (true) { … }`
in `Update` — is cut by fuel, resumes next frame, is cut again, and never
finishes; and while it stands there nothing else of its behavior runs. The
**watchdog** bounds the body. Per instance, it counts the game time the body
under way has spent *running* without finishing:

- a frame in which the body ran — it started or was resumed, spent fuel and was
  cut — adds that frame's delta, and so does a frame its behavior sits out
  repaying fuel *that body* overdrew, counted as it passes: that budget was
  spent in advance. A paused game, at a delta of zero, adds nothing;
- a frame it waits on an `await` adds nothing — that is waiting, not running —
  and neither does a frame its instance did not run for any other reason: a
  frame with no fuel for scripts, another instance's debt, disabled, or not
  playing;
- resuming from an `await` starts the count again from zero; finishing ends it.

Every body counts — `Update`, a timer, an `async` member, `OnSpawn`, `OnLoad`,
`OnResumeFailed` — in behaviors and systems alike. At `WATCHDOG_WARN`, **10 s**,
the body is reported once — a warning naming the entity, the behavior, the
member and the site it stands at, and an entry in the lane's report. At
`WATCHDOG_FAULT`, **60 s**, it **faults** like any fault: the body is dropped
— a timer's waits its interval again, and a body a hook stood in front of is
next — the instance disabled, and the fault recorded in its state companion
under the code it ran, so an edit to its module re-enables it. The count is
saved with the body under way, so a save and a load do not reset it, and a hot
reload carries it, whatever the edit made of where the body stood.

## Degrading defers whole behaviors

A budget that will not cover every behavior forces a choice. The lane could give
each behavior a smaller slice — every guard thinks a little less, uniformly. Or
it could give each its full turn, in order, until the budget runs out, and defer
the rest.

**It defers.** A behavior thinned every frame decides from a partial read of the
world, and a hundred of those is a hundred subtly wrong decisions that no one can
debug. A deferred behavior acts a frame late, which gameplay absorbs — a frame
late *is* what a frame is. The one behavior the budget runs out *inside* stops at
a safepoint and finishes its turn next frame, from where it stopped.

Who is deferred is fair, and what the author declared bends it:

- each behavior that can spend has an **equal share** of the frame's fuel, in its
  fixed order — what one does not spend passes on; a behavior that overdraws at a
  safepoint repays it from its next shares; each starts from a **cursor**, the
  first of its instances the last frame had no fuel for, so none is left out
  every frame; a turn is not started on less than half of what one costs;
- `[Critical]` claims first, `[Budget(d)]` caps, `[Rate(f)]` thins out turns
  ([Scheduling](../../reference/ergon.md#scheduling));
- **no time is lost**: a turn is handed all the time since the instance's last,
  so a timer, a wait or an `Update` integrating `dt` keeps time through the
  frames it sat out. Adapting the budget changes *when* a behavior runs, never
  how much game time it sees.

This is why `report_status` reports the share of behaviors that got their turn
rather than a simple success flag: an agent claiming perfect health while half
its behaviors were deferred would tell the DCC nothing was wrong at exactly the
moment something was.

## A fixed clock

`FixedUpdate(float dt)` runs once per tick of the game's fixed clock the frame
took — 1/60 s each, whatever precision physics sub-steps at — `dt` the tick,
inside the behavior's turn: after its timers, before its events and `Update`.
It is a clock, not a callback interleaved with physics: every call of a frame
sees the frame's view of the world, and its writes reach physics next frame.
Steps the fuel does not reach are owed, each with the step it was taken at, and
run first next turn; the count a game integrates does not depend on the budget.

## Scripts run only while the game runs

The script agent is registered for one engine mode: `Playing`. In the editor,
**editing** is the editor's own mode, and no script runs while you arrange a
scene; Play switches to `Playing`, and **pause** keeps it — scripts still run, at
a delta of zero.

<div class="kp-figure-frame">

{{#include ../../images/editor/play-modes.svg}}

</div>

Play does not resume anything: every scripted entity arrives fresh and runs
`OnSpawn`. Stop puts the scene back as it was. A shipped game starts in the mode
its `EngineApp::initial_mode` says — `Playing` unless it says otherwise.

Not running is not absent. The editor mounts the open project's scripts when it
opens it, and a save recompiles the module while you edit as while you play.
Scripts live in one folder, `assets/scripts/`: a module is named by its path
from there, and imports are written from there. An `.erg` saved anywhere else
is not a script — not at startup, not on a save — and says so. What
a script **declares** registers at once — the Inspector lists the project's
components and behaviors, and edits them as typed fields (a `Duration` in
seconds, an `Angle` in degrees) — and what it **compiled to** waits for Play.
What goes wrong is not left in the log: an `.erg` outside `scripts/`, compile
errors, a component two modules declare, a component no module declares any more, a faulted instance and a body
the watchdog warns about all land in one place, `ScriptProblems`, which the
editor shows as its **Problems** tab ([Editor — Scripts](../../reference/editor.md#07--scripts)).

---

*Next: [Changing the world](./writing.md). Back to [Scripting](./index.md).*
