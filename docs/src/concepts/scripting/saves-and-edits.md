# Saves and live edits

<p class="kp-lede">Save a game while a guard is half-way through swinging its sword, and loading it finishes the swing. Edit the guard's script while the game runs, and every guard keeps its health and what it was doing. Both work because the engine writes down what a script is in the middle of by <em>name</em> — <em>the guard was waiting at the line that says</em> <code>await 0.5s</code> — never by a position in memory, and a name survives an edit.</p>

## Saved mid-sentence

A behavior is a component. `Guard` — declared by its script, attached by adding
it to an entity — holds the behavior's fields: what the author set, and what the
game makes of them as it plays. A scene holds it as authored. Beside it, its
**state companion** `Guard#state` is **runtime and resumable**: what the engine
keeps to resume the running behavior. A scene never holds it; a
[game save](../saves.md) does, with the fields the game changed:

| A save keeps | So that, after a load… |
|---|---|
| the fields the game changed, by name — a `null` too (`Guard`) | the guard has the health it had; `best = null` loads as `null`, not as its default |
| the current state and its data | it is still chasing whom it was chasing |
| each timer's remaining time | `every 2s` fires when it would have |
| the body under way, frozen | an attack half-way through its wind-up finishes it |
| whether it has spawned | `OnSpawn` never runs twice |
| a fault, and the code it faulted under | a broken behavior stays off until its code changes |

A frozen body is written in names, not positions: its functions by name, its
text by value — a literal and text built while running alike — each frame by the
site it stands at, with its locals by name.
That is what lets a save made today load into the script as it is next month.

After a restore, `OnLoad` runs first; the restored body waits behind it.

A system is saved the same way, on [its one entity](./behaviors-and-systems.md#its-one-entity): the save
holds that entity, its fields and its companion — its timers, a sweep it was
part-way through, a fault. A scene never does.

## Edits while running

Hot-reload replaces a module's program while the game runs. Two things must
survive it: what each instance *is*, and what each instance is *doing*.

A module can also go: its file deleted, its folder moved out of `scripts/`,
its project closed. Its program leaves with it — nothing keeps running code
whose file is gone — and every instance of it stops, says so in Problems, and
keeps its fields. When the module comes back, by a move or an undo, its
instances carry those fields across as for any edit.

### Fields, by name

An author edits a guard's `Update` while ten guards are patrolling. The ten
should still be where they were, with the health they had — restarting them
would make the feature useless for exactly the thing it is for.

Fields are carried across a recompile and matched **by name**. A slot means
nothing across a rebuild: inserting one field at the top shifts every slot after
it, and a positional carry-over would silently move one guard's health into
another's ammo. A field that was renamed keeps no value, and the engine says so
in a warning rather than letting the author find out from whatever the guard
does next. A field the edit retyped keeps its value only if the new type can
hold it: `int? best` holding `null`, edited to `int best = 4`, starts at `4` —
never at a value no author could write. A load into edited code follows the same
rule.

How the inspector shows a field — its `[Range]`, `[Tooltip]`, `[Header]` — is
not the field: it is the editor's, read by nothing a script runs. An edit that
changes only those reaches the inspector at the next reload and leaves every row
as it was; a range bounds what the inspector edits, never what a script writes,
so a guard whose script pushes its health past the range keeps that health.

What the guard is *doing* crosses the same way: the state it is in by name, that
state's data by slot name, each countdown by its schedule's identity. A chasing
guard is still chasing after the edit, with what is left of its timers; a state
or schedule the edit removed is dropped, a new one starts fresh, and a datum new
to the current state starts at its own declared default.

The World and the program move **in the same frame**. An edit's new fields are
laid out in the World at the start of a frame; whatever compiled the program
announces what it reads, so the frame projects those components and the lane
installs the new program before any turn — no turn runs the old code over rows
already laid out for the new. When a program does have to wait a frame for what
it reads, its module's instances take no turn that frame: their events wait,
and an entity arriving then spawns under the new code. A behavior moved to
another module is carried there at its next turn, by the same rules, and an
instance carried while starved of fuel still has its record — what a save
reads — brought up to the new code that frame.

A system is carried by the same rules: its fields by name, a sweep cut
part-way rebuilt at its site in the edited body. An edit may also change what a
name *is*. A behavior turned into a system runs once a frame on the system's
entity; the entities that held the behavior keep their rows as data and run
nothing, and an authored one stays in the scene. A system turned back into a
behavior gives up its entity, and the behavior's holders run again from their
rows. A system renamed is a new system: the old name's entity goes, and the new
one starts from its defaults.

### A body part-way through, in tiers

A guard is half-way through `Attack`, stopped at its `await`, when the author
edits the script. What the frozen machine holds is a position in the old code.
Resuming it blindly in the new code would run whatever now sits at that
position; abandoning it would lose the attack every time anyone touches the
file. Khora does neither. It takes the body back in the best of five tiers:

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../../images/scripting/resume-tiers.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>Exact.</strong> The program did not change, or only a name did: the body resumes as it was.</li>
<li><strong>Unchanged.</strong> The program changed, but not the functions this body stands in: it resumes as it was.</li>
<li><strong>Rebuilt.</strong> Its function changed, but the place it stood still exists: it is laid out again there, each local found by name.</li>
<li><strong>Restarted.</strong> The place is gone, the member is still there: it runs again from its start, with the arguments it was first given.</li>
<li><strong>Abandoned.</strong> The member is gone, or takes other parameters: the body is dropped, and the behavior hears <code>OnResumeFailed(member)</code> to tidy up.</li>
</ol>
</figure>

- **Exact** and **unchanged** rest on fingerprints. Each function's fingerprint
  is a stable hash of its code with everything it names — a callee, a literal, a
  field — hashed by name, so moving another function or adding a field leaves
  it alone. Renaming a local changes no code at all: it resumes exactly.
- **Rebuilt** rests on site names. A statement's name comes from what it says,
  and an `if` or a loop from its header only, so adding a line above the
  `await`, or editing the loop body around it, leaves the `await`'s site where
  it was. Each frame is laid out again at its site in the new code, every local
  moved to its new register by name, declaring block and type.
- **Restarted** reruns the member with the arguments it was first given — not
  what its parameters held when it stopped, since a body may reassign them. What
  ran before the cut may run again; the author edited the code it was running.
- **Abandoned** tells the behavior, which can repair what the lost body was in
  the middle of:

```text
void OnResumeFailed(string member) {
    if (member == "Chase.Attack") { become Patrol; }
}
```

The same tiers apply to a game save loaded into edited code, and every resume
below *exact* is listed in the lane's report — which body, which tier.

A call with named arguments keeps this promise too. Its arguments run in the
order they are written, each into a register of its own, and only then are
placed in the order the callee declares its parameters. A turn stopped between
two of them holds what it evaluated whatever that order is, so an edit that
reorders the callee's parameters cannot hand one value to the other's name. A
call that names nothing compiles exactly as it always did.

<div class="kp-figure-frame">

{{#include ../../images/scripting/named-arguments.svg}}

</div>

---

*Next: [Under the hood](./under-the-hood.md). Back to [Scripting](./index.md).*
