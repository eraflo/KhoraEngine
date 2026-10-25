# Behaviors and systems

<p class="kp-lede">There are two ways to give a game its rules. A <strong>behavior</strong> is the brain of one entity — this guard, this door. A <strong>system</strong> is one rule over many entities at once — <em>every wounded player heals a little each second</em>. Same language, same tools; which one to write depends on whether the rule is about one thing, or about everything that matches.</p>

| Write a… | When the rule is about… | Example |
|---|---|---|
| behavior | one entity, over time — what it is doing, what it waits for | a guard's patrol, a door that opens, a boss's phases |
| system | every entity holding some components, the same way | healing, gravity zones, scoring |

## A behavior at a glance

A script declares **behaviors**: state and logic attached to an entity.

```text
event Damaged(int amount);
event Look;
event Spotted(Entity foe);

behavior Guard {
    int health = 100;

    void OnSpawn() { Log("on duty"); }

    on Damaged(int amount) {
        health -= amount;
        if (health <= 0) { this.Despawn(); }
    }

    state Patrol {
        every 2s { this.Raise(Look()); }
        on Spotted(Entity foe) { become Chase(foe); }
    }

    state Chase(Entity foe) {
        async void Attack() {
            await 0.5s;           // wind up
            foe.Raise(Damaged(10));
        }
    }
}
```

Fields hold what the guard *is*; lifecycle members (`OnSpawn`, `Update`, …)
are called by the engine; `on` handlers answer events, each one declared with
`event` so that a raise and every handler are checked against it; `every` and `after` are
schedules; `state` confines data and members to a phase of the behavior's life;
`async` members may `await` a duration. The [reference](../../reference/ergon.md)
lists every construct and what compiles today.

## A system at a glance

A behavior is one entity's logic over time. A **system** is one rule over every
entity a query matches:

```text
component Health { float current = 100.0; float max = 100.0; }
component Regen  { float rate = 2.0; }
component Shield { float bonus = 1.0; }
component Player { int team = 0; }
component Dead   { float since = 0.0; }

system Regeneration {
    float healed = 0.0;

    void Update(float dt) {
        foreach (Entity e, ref Health h, Regen r, Shield? s) with Player without Dead {
            float rate = r.rate;
            if (var shield = s) { rate += shield.bonus; }
            float before = h.current;
            h.current = Min(h.max, h.current + rate * dt);
            healed += h.current - before;
            if (h.current >= h.max) { e.Remove(Regen); }
        }
    }

    every 10s { Log("regenerating"); }
}
```

A system is **a behavior with no subject**. It has fields, `Update`, `every`
and `after`, methods, `OnLoad` and `OnResumeFailed` — and nothing that would need
an entity of its own. The checker refuses, at the construct and saying why:

| Refused in a system | Because |
|---|---|
| `this`, `Position()`, a free function that reads `Position()` | there is no entity it is attached to — its entities are the rows of its queries |
| `state`, `become` | what one entity is doing is a behavior's state |
| `on` handlers | an event is aimed at an entity |
| `async`, `await` | a wait would be one entity's time; a system's time is `every` and `after` |
| `OnSpawn`, `OnDespawn` | a system starts with the game, not with an entity |
| attributes | `[Critical]`, `[Budget(…)]`, `[Rate(…)]` schedule it like a behavior; never `[Domain(…)]` — its fields live in the `Script` domain |

### Its one entity

Under the hood a system runs **as a behavior instance** — so its fields, its
timers, its share of fuel, a body cut short, a fault, a hot reload and a game
save are the behaviors' machinery, not a second one. An instance needs an
entity, and nobody authors a system's: while the game plays, the
`script_systems` data system keeps **one entity per system**, known by a named
identity — `PersistentId::named("system:Regeneration")` — and holding the
system's component (its fields) and its state companion
`Regeneration#state`. Both are `Runtime` and resumable:

- **No scene holds it**, whatever the encoding — and Play's snapshot neither, so
  Stop takes it away. An authored entity that happens to hold a system's
  component is still the author's.
- **A game save keeps it**, with its fields and timers; a load finds the entity
  of that name again instead of making a second.
- **No script reaches it.** `Regeneration` is not a component a script may
  `Get`, `Set`, `Add`, `Remove`, `Has` or query: any other holder would be a
  second instance, run every frame beside the first. A system's fields are its
  own, named directly.
- It exists while gameplay runs, and goes when no script declares the system
  any more. A system imported by several modules runs once a frame — from the
  module that declares it.

---

*Next: [Turns and time](./turns.md). Back to [Scripting](./index.md).*
