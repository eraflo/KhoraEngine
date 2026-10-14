# Ergon reference

The Ergon language as it compiles and runs **today**: every construct, with an
example, and an honest line on what parses but does not run yet. For how a
script runs inside a frame — turns, safepoints, saves, edits — see
[Scripting](../concepts/scripting.md).

Each construct is marked:

| Mark | Means |
|---|---|
| **runs** | compiles and runs, pinned by the conformance suite |
| **checks** | the type checker accepts it; the compiler refuses it — "cannot be compiled yet" |
| **parses** | accepted by the parser, refused or ignored after |

The conformance suite (`crates/khora-script/tests/it/conformance.rs`) is the
source of truth: every *runs* row gives the same result when its execution is cut
at any fuel slice from 1 to 60.

---

## A file

```text
import "combat/damage.erg";

fn int Double(int n) { return n * 2; }

behavior Guard {
    int health = 100;
    on Damaged(int amount) { health -= Double(amount); }
}
```

| Item | Status | Notes |
|---|---|---|
| `import "path.erg";` | runs | before every other item; paths relative to the script root; a cycle is refused with its chain. Every imported module joins **one flat namespace**; `as Alias` parses and is unused |
| `fn <type> Name(params) { … }` | runs | free functions; called before their declaration and recursively; `this` is refused in them; a name an engine function already has is refused |
| `behavior Name { … }` | runs | a component of the same name, its fields the behavior's, with a state companion `Name#state`; attached by adding the component (`GameWorld::add_declared_component`). Names are global: two modules declaring one is refused |
| `system Name { … }` | runs | a behavior with no subject, run once a frame on an entity of its own; see [Systems](#systems). Its name is global like a behavior's |
| `struct Name { … }` | runs — a value type; its field defaults must be constants; operator overloads check but do not compile yet |
| `[Domain(D)] component Name { … }` | runs — a component the project declares; see [Declaring components](#declaring-components) |
| `[Domain(D)]` on a behavior | runs — where the behavior's fields are stored (default `Script`) |
| `[Critical]`, `[Budget(…)]`, `[Rate(…)]` on a behavior | parses — nothing reads them yet; any other attribute is refused |

Comments are `// line` and `/* block */`.

## Values and types

| Literal | Forms | Type |
|---|---|---|
| integer | `42`, `0x2A`, `1_000` | `int` (64-bit) |
| float | `2.0`, `1e-3` | `float` (32-bit) |
| duration | `2s`, `500ms`, `1.5min` | `Duration`, stored in seconds |
| angle | `90deg`, `1.57rad` | `Angle`, stored in radians |
| text | `"text"` | `string` |
| boolean, none | `true`, `false`, `null` | `bool`, `T?` |

| Type | Status |
|---|---|
| `int`, `float`, `bool`, `string`, `Entity` | runs |
| `Vec2`, `Vec3`, `Vec4`, `Quat`, `Color` | runs — built by calling `Vec3(1.0, 2.0, 3.0)`; components `.x .y .z .w` (`.r .g .b .a`) read-only |
| `Duration`, `Angle` | runs — distinct from `float` and from each other; `2s + 500ms` is `2.5s`, `2s + 90deg` is refused |
| `T?` | runs — holds a value or `null`; one declared without a value starts `null`; `a ?? b`, `?.`, `if (var x = opt)`, `while (var x = opt)` and `match` unwrap it |
| `T[]` | runs — `[1, 2, 3]`, `xs[i]`, `xs.Length`, `foreach`, `xs.Push(v)`, `xs.RemoveAt(i)`; one declared without a value starts `[]` |
| `Map<K, V>` | checks — no literal, no indexing |

The one implicit conversion: an `int` where a `float` is expected. A
`Duration` or an `Angle` reads as a `float` with `d.Seconds`, `a.Radians`,
`a.Degrees`.

## Arrays and structs

```text
struct Waypoint { Vec3 at; float wait = 0.5; }

behavior Patroller {
    Waypoint[] route = [
        Waypoint { at: Vec3(0.0, 0.0, 0.0) },
        Waypoint { at: Vec3(4.0, 0.0, 0.0), wait: 2.0 }
    ];
    int current = 0;

    void Update(float dt) {
        Waypoint next = route[current];      // a copy
        current = (current + 1) % route.Length;
    }
}
```

- **Values, not references.** Binding an array or a struct read from a
  variable, a field or an element copies it — `b = a; b.x = 9;` leaves `a`
  alone, and so does passing one to a function or iterating it. Two guards never
  share one patrol route by accident; to share data, point at the same entity.
- **Writing through a path changes the value in place**: `route[1].at = …`,
  `grid[0][2] = 7`, `xs[i] += 1`.
- **A literal names its fields**, in any order: `Waypoint { at: p }`. A field it
  leaves out takes its default, else its type's zero; a field with neither must
  be written. A struct's defaults are constants — a number, text, `null`, a
  duration, an angle, or an array or struct literal of those.
- **Growing and shrinking**: `xs.Push(v)` appends a copy of `v`;
  `xs.RemoveAt(i)` removes the element at `i` and shifts the rest down. Both
  change the array in place, so `xs` must be a variable, a field, or an element
  or field of one — `Make().Push(1)` is refused: nothing would keep the result.
- `xs[i]` outside the array faults (`IndexOutOfRange`). `==` is not defined on
  arrays or structs — compare what they hold. `Length` and an engine type's
  components are read-only.
- `s?.field` reads a field of an optional struct, or gives `null`.
- **Saved by name.** A field holding an array or a struct is saved and
  restored by its field names: after an edit, a struct field that is gone is
  dropped, a new one takes its default or its zero.
- Copying, building and saving cost fuel in proportion to the size.

## Fields

```text
behavior Lamp {
    float brightness = 0.8;
    Color tint = Color(1.0, 0.9, 0.7, 1.0);
    int lit;            // starts at 0
}
```

A field is `Type name [= default];` — `var` is for locals only. A default is a
**constant** — a number, text, `null`, a duration, an angle, an engine value of
numbers (`Vec3(0.0, 1.0, 0.0)`), or `[…]` / `Name { … }` of those — because a
behavior is a component and its defaults are data: what its component registers,
what a fresh instance holds before its first turn, and what an edit that adds the
field gives a running one. No code runs for them. A starting value that has to be
computed — `home = Position();` — is set in `OnSpawn`. Without a default, a field
starts at its type's neutral value — `int` `0`, `float`, `Duration`, `Angle`
`0.0`, `bool` `false`, `string` `""`, `T?` `null`, `T[]` `[]`, `Vec2`/`Vec3`/`Vec4`
zero, `Quat` the identity, `Color` white, a struct its own defaults. An `Entity`
has none: write it `Entity?`, and it starts at `null`. A state's data follows the
same rule; a value computed on entering a state is a parameter of `become`.

## Members

| Member | Form | Status |
|---|---|---|
| method | `int Speed() { return 3; }` or `int Speed() => 3;` | runs |
| async method | `async void Attack() { await 0.5s; … }` | runs |
| event handler | `on Damaged(int amount) { … }`, `on Lost => become Patrol;` | runs — parentheses optional without parameters |
| schedule | `every 0.5s { … }` | runs — first fires one full interval in |
| one-shot | `after 10s { … }`, `after 10s => become Patrol;` | runs — fires once |

A member calls a sibling by its bare name. Inside a state, that state's own
methods come first, then the behavior's, then free functions; outside every
state, the behavior's, then free functions. A state's method exists only inside
its state: called from anywhere else, the checker refuses it and names the state
it belongs to. A schedule's interval must be a **literal duration** — a field or
an expression is refused.

### Lifecycle members

The engine calls these by exact name. The checker requires `void` and exactly
these parameters:

| Signature | When |
|---|---|
| `void Update(float dt)` | every turn, last, with the frame's delta |
| `void OnSpawn()` | once per entity, ever — a save remembers it ran; a hot reload does not rerun it |
| `void OnLoad()` | after a game save restored the instance, before anything else; never on a scene load or Stop |
| `void OnDespawn()` | when the despawn is decided — by `this.Despawn()`, or noticed a frame later |
| `void OnResumeFailed(string member)` | an edit left nothing of a body part-way through; `member` is e.g. `"OnSpawn"`, `"Chase.Attack"`, `"__every(0.5)"` |
| `void FixedUpdate(float dt)` | reserved — never called yet; the checker warns |

See [One turn](../concepts/scripting.md#one-turn) for their order. A
[system](#systems) takes `Update`, `OnLoad` and `OnResumeFailed`; it has no
entity to call `OnSpawn` or `OnDespawn` for.

## States

```text
behavior Guard {
    state Patrol {
        on Spotted(Entity foe) { become Chase(foe); }
    }
    state Chase(Entity foe) {
        int missed = 0;
        every 1s { foe.Raise("Damaged", 5); }
        after 10s => become Patrol;
    }
}
```

```mermaid
stateDiagram-v2
    direction LR
    [*] --> Patrol: first declared
    Patrol --> Chase: become Chase(foe)
    Chase --> Patrol: become Patrol
    note right of Chase
        foe and missed exist only here;
        its every and after run only here,
        re-armed on each entry
    end note
```

- An instance starts in the **first declared** state, entered with no
  arguments — so the first state takes no parameters.
- A state's parameters and fields exist only inside it; entering it — by
  `become` or at the start — writes its defaults and re-arms its schedules.
- A state's member wins over the behavior's member of the same name — for the
  engine's calls and for a call written inside the state; the behavior's is the
  fallback in every state. A state's defaults are its own code: they read the
  behavior's fields, the state's parameters and its fields declared above, and
  call its methods — wherever the `become` that enters it is written, never
  seeing that member's locals or the data of the state it is written in.
- `become Name;` / `become Name(args);` switches state and **does not end the
  member**: the statements after it still run.

## Statements

| Statement | Status |
|---|---|
| `int x = 1;`, `var x = 10;` | runs — `var` needs an initialiser |
| `x = …`, `+=`, `-=`, `*=`, `/=` | runs — no `%=`, `++`, `--` |
| `if` / `else`, `else if` | runs — braces optional around one statement |
| `while`, C-style `for` | runs |
| `return;`, `return x;`, `{ … }` | runs — a function returning a value must `return` on every path: `if` counts with an `else`, `match` when every arm does, a loop never |
| `become` | runs |
| `await duration;` | runs — in an `async` member only |
| `break`, `continue` | runs — act on the innermost loop, from anywhere inside it; in a `for`, `continue` still runs the step |
| `if (var x = opt)`, `while (var x = Next())` | runs — `x` is the present value, in the branch or the body only |
| `match (opt) { T x => …, null => … }` | runs — takes an optional apart; see below |
| `foreach (var x in xs)`, `foreach (int x in xs)` | runs — over a copy of `xs` taken when the loop starts; `x` is a copy of each element |
| `foreach (Entity e, ref Health h, Regen r) with A without B` | runs — a query: once per entity holding what it names; see [Queries](#queries) |
| `int x;` without a value | runs — starts at its type's zero; `Entity e;`, `Vec3 v;` are refused: give them a value, or use `Entity?` |

### `match`

`match` takes an **optional** apart; it is not a `switch`. Its patterns are
`null`, `Type name` (the present value, bound for the arm) and `_`:

```text
match (target) {
    Entity e => Chase(e),
    null => Patrol(),
}
```

The subject is evaluated once; the first arm that matches runs. An optional
subject needs both cases, or a `_`. An arm after one that already takes all it
could match is warned unreachable, and a `null` arm on a subject that is never
null is refused. There are no value patterns — `1 =>`, `"idle" =>` do not parse.

### Waiting

`await` is allowed in an `async` member, and waits on a **duration** — nothing
else yet. A member that is not `async` may still *call* one, and then the whole
call waits:

```text
on Spotted(int by) { Attack(); }          // the handler waits with it
async void Attack() { await 0.5s; Strike(); }
```

`await` itself is refused in `Update`, `on` handlers, `every` and `after`.

## Expressions

| Expression | Status |
|---|---|
| `* / %`, `+ -`, `< <= > >=`, `== !=`, `&&`, `\|\|`, `!`, unary `-` | runs — C# precedence; `&&`/`\|\|` short-circuit; `int / int` truncates |
| `c ? a : b` | runs |
| `"a" + "b"`, `==` on text | runs — text `+` text only: `"hp " + health` is refused |
| `(float)x`, `(int)x` | runs — converts: `(float)5` is `5.0`, `(int)2.75` is `2`, `(int)-2.75` is `-2` (toward zero); `(int)` of an infinity, a NaN or a value past `int`'s range faults (`InvalidCast`) rather than saturating |
| `this` | runs — the behavior's entity; not in free functions |
| `Name(args)` | runs — functions and natives by bare name |
| `obj.Method()` | checks |
| `a ?? b` | runs — `a` unless it is `null`; `b` is evaluated only then. A `float?` falls back to a float even when `b` is written as an int |
| `[a, b]`, `a[i]`, `a.Length`, `a?.b`, `Name { field: v }` | runs — see [Arrays and structs](#arrays-and-structs) |
| `new T(…)` | refused — not supported yet; build an engine type with its function, `Vec3(1.0, 2.0, 3.0)` |
| `entity.Field` | refused — an entity has no fields: a behavior's own fields are named directly, a component is read with `Get` |

## Engine-type arithmetic

An engine type's operators are engine functions, exactly as `v.x` is: an
operation is defined when the engine defines it, and refused at compile time —
by name — when it does not.

| Type | Operators |
|---|---|
| `Vec2`, `Vec3`, `Vec4` | `v + w`, `v - w`, `v * s`, `s * v`, `v / s`, `-v` — `s` a number, an `int` widened |
| `Quat` | `q * r` composes, `q * v` rotates a `Vec3` |
| `Color` | `c + d`, `c - d`, `c * d` (modulate), `c * s`, `s * c` |

Compound assignment uses the same table: `v += w`, `v *= 2.0`. Anything else —
`Vec3 * Vec3`, `Vec3 % float`, `Quat + Quat`, `Color / float`, `2.0 / v` — is
refused with the operation's name and the list of what the type defines.

## Engine functions

| Area | Functions |
|---|---|
| Maths | `Abs`, `Floor`, `Ceil`, `Round`, `Sign`, `Sqrt` (faults below zero) `(float) → float`; `Min`, `Max` `(float, float)`; `Clamp(v, lo, hi)` (faults if `lo > hi`); `Lerp(a, b, t)` (unclamped) |
| Text | `Log`, `Warn`, `Error` `(string)`; `Length(string) → int` |
| World — queued, applied at the frame's end | `Position() → Vec3` (own entity, as the frame began); called on an entity `e`: `e.Translate(Vec3)`, `e.SetPosition(Vec3)`, `e.SetRotation(Quat)`, `e.SetScale(Vec3)`, `e.Despawn()`, `e.SetParent(Entity)`, `e.Detach()`; components — see [Writing components](#writing-components) |
| Input | `Pressed(string action) → bool`, `JustPressed(string) → bool` |
| Events | `target.Raise(string name, …payload)` |
| Constructors | `Vec2`, `Vec3`, `Vec4`, `Quat(x, y, z, w)`, `Color(r, g, b, a)` |

A world write is a queued command: `Position()` does not see a `SetPosition` until
the next frame.

**An operation on an entity is called on it.** Every engine function whose first
parameter is an `Entity` is written `e.Name(…)` — `this.SetPosition(v)`,
`enemy.Despawn()`, `target.Raise("Hit", 3)`. The free form `SetPosition(this, v)` is
refused with its rewrite. `Spawn`, the maths, `Log` and `Pressed` stay free
functions.

## Writing components

```text
import "engine/components.erg";

behavior Anchor {
    void OnSpawn() {
        this.Set(RigidBody { mass: 12.0 });   // only `mass` changes
        this.Add(Collider { friction: 0.8 }); // defaults, then `friction`
    }
    on Released() { this.Remove(Collider); }
}

behavior Spawner {
    every 2s { Spawn(Position(), AudioSource { volume: 0.5 }); }
}
```

| Write | Does | Refused |
|---|---|---|
| `e.Set(C { field: v, … })` | writes the fields named; the others keep their values | at the frame's end, when `e` has no `C` |
| `e.Add(C { field: v, … })` | attaches `C` with its defaults, then writes the fields named | at the frame's end, when `e` already has a `C` |
| `e.Remove(C)` | detaches `C` | at the frame's end, when `e` has no `C` |
| `Spawn(Vec3 position, C1 { … }, …)` | a new entity at `position`, upright, carrying each component | the whole spawn, when one component refuses its value — no entity is left behind |

- The components a script writes are those `engine/components.erg` declares with
  `component` — the ones an author may add in the editor — and the project's own
  ([Declaring components](#declaring-components)). `Transform` is placed with
  `SetPosition`, `Translate`, `SetRotation` and `SetScale`; a component the engine
  computes (`Derived`, `Runtime`) or a tool sets (`Parent`) is a `struct` there — a
  value a script can build, not write.
- A component literal is a *patch*: it appears only as the argument of `Set`, `Add`
  or `Spawn`, and its fields are evaluated in the order written. A component is not
  a type — `RigidBody r;` is refused.
- Each write costs `1 +` the number of fields it names. `Spawn` returns nothing:
  the new entity configures itself in its own `OnSpawn`.
- `Set`, `Add`, `Remove`, `Get`, `Has` and `Spawn` are reserved: no function or
  method may take one of these names.

## Reading components

```text
if (var h = target.Get(Health)) {
    if (h.current < 20) { target.Set(Health { current: h.current + 1 }); }
}
bool shielded = target.Has(Shield);
```

| Read | Gives |
|---|---|
| `e.Get(C)` | `C?` — a copy of `e`'s component, or `null` when it has none |
| `e.Has(C)` | `bool` |

- Any entity, any `component`. A copy is read-only: `h.current = 5` is refused —
  write the component with `e.Set(C { … })`. `C` names the type of a local given
  its value (`Health? h = e.Get(Health);`), never a field, a parameter or a return.
- **A script sees its own writes within the frame.** `Set` then `Get` reads the new
  value; `Add` makes `Has` true and `Get` the defaults with the fields written;
  `Remove` makes them false and `null`. The engine's systems see only what the
  frame's end applied. A despawned entity keeps its components until the frame's end — its
  `OnDespawn` still reads them.
- Only the components some loaded program reads are projected each frame, and
  nothing when none reads anything; a `Get` converts only the row it reads. It
  costs `1 +` the component's field count; `Has` costs 1.
- A reload that reads a component no program read before runs from the next frame,
  the first whose view holds it — its first `Get` never reads `null` for a
  component that exists.

## Declaring components

```text
[Domain(Physics)]
component Health { int current = 100; int max = 100; }

component Loot { string item = ""; int value = 0; Vec3 drop = Vec3(0.0, 1.0, 0.0); }

behavior Chest {
    on Opened() {
        Spawn(Position(), Loot { item: "sword", value: 30 });
        this.Remove(Loot);
    }
}
```

A project's `component` is a real component: stored in its domain's pages, in a
packed column, written and read with `Set`, `Add`, `Remove`, `Spawn`, `Get` and
`Has` like the engine's, saved with scenes, prefabs and game saves.

- `[Domain(D)]` places the data — `Spatial`, `Render`, `Audio`, `Physics`, `Ui`
  or `Script`; without it, `Script`. Only one, and no other attribute.
- A field without a default takes its type's zero (`0`, `0.0`, `false`, `""`,
  `null`, `[]`). An `Entity`, a struct or an engine value has none: give it a
  default or make it optional. A default is a constant — an engine value may be
  built from numbers, `Vec3(0.0, 0.0, 1.0)`.
- A field holds what a struct field holds; a `Map` is refused. A name used twice
  is refused.
- A name an engine component has — or had (`formerly`) — is refused at the
  declaration, whether the module imports `engine/components.erg` or not. Two
  modules declaring one name: the first keeps it, the second is refused at
  registration and logged.
- The engine registers a declaration at the frame boundary, in the editor and in
  play alike. Editing it while the game runs:

| The author… | The rows already stored… |
|---|---|
| adds a field | take its default |
| renames or removes a field | lose its value, logged by name; a new name takes its default |
| changes a field's type | take its default — except `int` → `float`, kept where it converts exactly |
| changes `[Domain]` | move to the new domain's pages, values intact |
| deletes the declaration | keep it, with their data: the component is *orphaned*, logged, never deleted — declaring it again adopts it |

- A scene holding a project component describes it — its domain and fields — so
  it loads anywhere, before any script runs, and still loads once the project
  stops declaring it (orphaned). A world declaring it differently reads the
  scene's fields onto its own by name, and the load report says what it adapted.

## Events

```text
on Damaged(int amount) { health -= amount; }
void Hurt() { this.Raise("Damaged", 30); }
```

- `target.Raise("Name", …)` delivers **next frame**, to the handler named `Name`
  — the current state's first, then the behavior's.
- The payload is checked against the handler that receives it: a wrong count is
  reported, a missing handler is simply not delivered, a despawned target hears
  nothing.
- A payload carries `bool`, `int`, `float`, `string`, `Entity` and the engine
  types — not `null`, whether a script or the engine raised it.
- The engine raises `Touched(Entity other)` and `Separated` from collisions:
  `on Touched(Entity other) { other.Despawn(); }`.

## Systems

```text
component Regen { float rate = 2.0; }

system Spawner {
    int spawned = 0;

    every 2s {
        Spawn(Vec3(0.0, 0.0, 0.0), Regen { rate: 1.0 });
        spawned += 1;
    }
    after 30s { Log("half a minute in"); }
    void OnLoad() { Log("resumed"); }
}
```

A `system` is a behavior with no subject: one rule over the entities its
[queries](#queries) match, run once a frame while the game plays, on an entity
of its own that no scene holds and a game save keeps
([its one entity](../concepts/scripting.md#its-one-entity)). Its members are a
behavior's — fields, `Update`, `OnLoad`, `OnResumeFailed`, `every`, `after`,
methods — and its fields are named directly. In its queries, and only there, a
binding may be `ref`.

| Refused in a system | Message, in short | Why |
|---|---|---|
| `this` | `this` names the entity a behavior is attached to, and a system has none | its entities are the rows of its queries — bind one with `Entity e` |
| `Position()` | `Position()` reads the entity a behavior is attached to | the same; there is no own position |
| a call to a free function that reads `Position()`, itself or through others | `F` reads the entity a behavior is attached to through `Position()` | refused at the call; pass the function an `Entity` |
| `state S { … }` | a system has no states | what one entity is doing is a behavior's state |
| `become S;` | `become` has nowhere to go in a system | the same |
| `on E(…) { … }` | a system receives no event: `on E` is aimed at an entity | handle it in a behavior of the entities it is sent to |
| `async` member | a system has no `async` member | a wait would be one entity's time; a system's time is `every` and `after` |
| `await` | `await` is not allowed in a system | the same |
| `OnSpawn`, `OnDespawn` | a system has no entity to call `OnSpawn` for | a system starts with the game |
| `[Critical]`, `[Domain(…)]`, any attribute | attributes only apply to behaviors and components | its fields live in the `Script` domain |
| a name another declaration, an engine component or its `formerly` has | declared twice, or an engine component | a system is a component: one namespace for all |

A system's component is not one a script reaches: `e.Get(Spawner)`,
`e.Set(Spawner { … })`, `Add`, `Remove`, `Has` and a query binding `Spawner`
are refused as not a component a script writes or binds — another holder would
be a second instance.

`system` and `ref` are keywords. `with` and `without` are words only after a
query's parentheses; anywhere else they are ordinary names.

## Queries

```text
component Health { float current = 100.0; float max = 100.0; }
component Dead   { float since = 0.0; }

system Triage {
    int critical = 0;

    void Update(float dt) {
        foreach (Entity e, ref Health h) without Dead {
            if (h.current <= 0.0) { e.Add(Dead { }); continue; }
            h.current -= 1.0;
            if (h.current < 10.0) { critical += 1; }
        }
    }
}

behavior Medic {
    every 1s {
        foreach (Entity e, Health h) without Dead {     // read-only
            if (h.current < 20.0) { e.Set(Health { current: h.current + 5.0 }); }
        }
    }
}
```

`foreach (bindings) [with A, …] [without B, …] body` — `with` before
`without`, each optional. A row is an entity holding every component bound
without `?` and every `with`, and none of the `without`.

| Binding | Holds |
|---|---|
| `Entity e` | the row's entity; at most once, never `?` or `ref` |
| `C c` | a read-only copy of the row's `C` |
| `C? c` | a copy, or `null` where the row has no `C`; it does not filter |
| `ref C c` | a copy whose changed fields are written back at the row's end — in a system only |

- **The rows are taken when the sweep starts**, in entity order, from the
  frame's working set — this frame's `Set`, `Add` and `Remove` included. A
  structural change in the body is a command and leaves the sweep alone; each row
  is checked again when it is reached, and skipped if it no longer matches.
- **A `ref` copy is written where the row ends** — after the body, where a
  `continue` lands, before a `break` or a `return` leaves the sweep: its changed
  fields reach the working set at once and leave as one column write per
  component and set of fields, in the order the run made them. See
  [Queries and sweeps](../concepts/scripting.md#queries-and-sweeps).
- `break`, `continue` and `return` act as in any loop; a query inside a query
  sweeps its rows once per outer row.
- **Cost**: taking the rows `1 +` the candidates visited (the holders of the
  rarest required component), and is a place a run may stop before; checking a
  row `1 +` the components it is checked against; each binding read `1 +` its
  fields (a `ref` binding twice: its copy, and the row as read); each `ref`
  write-back `1 +` the fields compared.
- Outside a system's own members — in a behavior, in a free function — a query
  is read-only.

| Refused | Message, in short | Why |
|---|---|---|
| `ref C` outside a system | this query is read-only: only a system writes `C` in place | a behavior writes other entities with `e.Set(C { … })` |
| `ref G`, `G` a behavior | `G` is a behavior's component: its fields are written by its own turn, or by `Set` | its turn builds on the `Set`s queued before it; a column write would bypass that order |
| `ref C? c` | a `ref` binding cannot be optional | where `C` is absent there is nothing to write back to |
| `r.rate = 2.0` on a plain binding | `r` is a copy of `Regen` and is read-only | write it with `e.Set(Regen { … })` |
| `h = …` on any binding | `h` is a query's binding and cannot be reassigned | it is the row's own — declare a local |
| `Loot l`, `int n`, `Health[] hs` | `X` is not a component | a query binds `Entity` and components |
| `Entity? e`, `ref Entity e` | a row's entity is always there, and is not written | bind it as `Entity e` |
| `Entity e, Entity f` | the row's entity is bound twice | — |
| `Health a, Health b` | `Health` is named twice in this query | a component is bound once |
| `Health h, Regen h` | `h` is bound twice in this query | — |
| `Health h` … `without Health` | `Health` is both named and excluded | no row could match; with `Health?`, its binding would always be `null` |
| only `?` bindings and `without` | a query needs a component every row holds | bind one without `?`, or name one after `with` |
| `with`/`without` naming a struct or an unknown name | `X` is not a component | — |

## Not yet

Maps, methods on structs and arrays (beyond `Push` and `RemoveAt`), struct
operator overloads at run time, value patterns in `match`, awaiting events,
closures, reading `Transform` with `Get` (use `Position()`), project components in
the editor's inspector, reordering systems (`[Before]` / `[After]`), change
filters in queries, systems running in parallel.

## See also

- [Scripting](../concepts/scripting.md) — turns, safepoints, saves, resuming
  after edits.
- [Glossary](./glossary.md) — safepoint, site, resume tier, system, query, sweep,
  watchdog.
