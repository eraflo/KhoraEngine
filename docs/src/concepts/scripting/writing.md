# Changing the world

<p class="kp-lede">A script never changes the game directly. It writes notes — <em>move me here</em>, <em>give that one a shield</em>, <em>make a new bullet</em> — and the engine carries them all out together once every script has had its turn, like a kitchen cooking the orders once the waiters have taken them. It sounds slower. It is what lets scripts run at the same time as the rest of the engine, and lets a script that goes wrong be undone completely.</p>

## A script never writes the world

Effects are queued as `WorldCommand`s and applied by a data system during
[Maintenance](../the-frame.md). Nothing a script does reaches the ECS while it
runs.

That constraint is not bureaucracy — it is what buys the parallelism. Because
the script lane touches no `World`, its agent declares `AgentAccess::Isolated`
and can share a wave with other world-free agents. The rule and the performance
are the same fact.

The commands apply in the order they were queued. Many rows of one component —
a system sweeping ten thousand healths — travel as **one** command, a column
write: the component and its fields are resolved once, the rows are written page
by page in memory order, and a row whose entity is gone is skipped and counted
rather than failing the rest. On 10,000 rows it costs about a sixth of the same
writes queued one by one (`cargo bench -p khora-data --bench data --
RigidBody.mass`).

Two writes landing on the same thing in one frame are reported in debug builds —
the last one wins. They are told apart **per field**: two scripts writing
`current` and `max` of one `Health` compose and are not reported; two writing
`current` are, and so is an add or a removal of `Health` beside any write to it.

A behavior is a component, so another script writes it like one —
`target.Set(Guard { speed: 2.0 })` — and reads it with `Get` and `Has`; a
behavior changes its own fields by assigning them, never by a `Set` of itself.
Its turn and the scripts around it write in **one order**, the order the frame
ran them in. A `Set` of a guard's field queued before the guard's turn is read
by that turn — `speed += 1` builds on it, and nothing is lost. A `Set` queued
after the turn is written after it and wins: the field the turn changed is
the `Set`'s, and the turn's write is reported as lost, like any other. A `Set` a
turn queues of its own fields — through an entity that happens to be its own —
was not read by it and loses to what the turn wrote, reported as well; a
component added or removed in the frame is another instance's, and keeps
nothing the turn wrote of the old one. A field the turn only read is not a write
of it, and each lost write is reported once, with every write that meant to set
the field counted.

### An entity named before it exists

`Spawn` returns the new entity at once, though nothing reaches the world until
the frame's end. The turn takes an id from the world's **reserver** — not alive
yet, handed to no one else — and the spawn's command carries it; what the turn
queues after it names it as any other entity:

```text
var b = Spawn(bullet, Damage { amount: 3 }, at: muzzle); // the instance's root
if (var d = b.Get(Damage)) { Log($"armed: {d.amount}"); }  // read in the same turn
b.Raise(Fired());                                          // queued after the spawn
```

The turn reads what it spawned from its working set: the components it gave
the spawn, its `Transform`, and every write after it. A prefab's own components
are read from the next frame, once the instance exists.

At Maintenance the ids are settled before the commands apply, and a spawn
commits its id only when **every** component accepts its value; one refusal
releases it, and the spawn never existed — no entity left behind, half-built.
Whatever was handed out and not committed goes back to the store at the end of
the same Maintenance, every frame, whether the scripts queued anything or not.
An index taken again is taken with a new generation, so an old `b` kept in a
field never names the entity that reuses it. A reservation never fails for
lack of ids: the reserver counts past the store's end when its ready ids run
out, and keeps as many ready as the last frame spent.

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../../images/scripting/reserve.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>Reserve.</strong> <code>Spawn</code> takes an id from the world's reserver. Nothing exists yet, but <code>b</code> already names it.</li>
<li><strong>Use.</strong> The turn reads back what it spawned and wrote, as if the entity were there.</li>
<li><strong>The turn ends.</strong> Finished or cut short, everything it queued stays. Faulted, nothing does, and its ids go back.</li>
<li><strong>Settle.</strong> At the frame's end, ids counted past the store's end become reserved slots.</li>
<li><strong>Apply.</strong> The commands run in order. The spawn commits its id only if every component accepts its value.</li>
<li><strong>Release.</strong> Every id handed out and never committed goes back, reused later with a new generation.</li>
</ol>
</figure>

### A faulted turn leaves nothing

A turn that faults — an index out of range, a `null` prefab, the watchdog —
takes back everything it queued **in that frame**: its commands, its column
writes, the events it raised, its working-set writes, and the ids of its spawns,
released unspent. The world is left as if the turn had not run this frame, as a
load that fails leaves the world as it was. What a body queued in earlier frames,
before it was cut short and resumed, was applied with those frames and stays.
The same holds for `OnDespawn`: a farewell that faults leaves nothing.

---

*Next: [Queries and sweeps](./queries.md). Back to [Scripting](./index.md).*
