# Ergon reference

The Ergon language as it compiles and runs **today**: every construct, with an
example, and an honest line on what parses but does not run yet. For how a
script runs inside a frame — turns, safepoints, saves, edits — see
[Scripting](../concepts/scripting/index.md).

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
| `import "path.erg";` | runs | before every other item; paths relative to `assets/scripts/`, the one folder scripts live in; a cycle is refused with its chain. Every imported module joins **one flat namespace**; `as Alias` parses and is unused |
| `fn <type> Name(params) { … }` | runs | free functions; called before their declaration and recursively; `this` is refused in them; a name an engine function already has is refused |
| `behavior Name { … }` | runs | a component of the same name, its fields the behavior's, with a state companion `Name#state`; attached by adding the component (`GameWorld::add_declared_component`). Names are global: two modules declaring one is refused |
| `system Name { … }` | runs | a behavior with no subject, run once a frame on an entity of its own; see [Systems](#systems). Its name is global like a behavior's |
| `event Name(params);` | runs | what a raise names and a handler takes; see [Events](#events). Its name is global; `event Name;` carries nothing |
| `struct Name { … }` | runs — a value type; its field defaults must be constants; operator overloads check but do not compile yet |
| `[Domain(D)] component Name { … }` | runs — a component the project declares; see [Declaring components](#declaring-components) |
| `[Domain(D)]` on a behavior | runs — where the behavior's fields are stored (default `Script`) |
| `[Critical]`, `[Budget(…)]`, `[Rate(…)]` on a behavior or a system | runs — how the script lane schedules it; see [Scheduling](#scheduling); any other attribute is refused |
| `[Range(…)]`, `[Tooltip(…)]`, `[Header(…)]` on a field | runs — how the inspector shows it; see [Field attributes](#field-attributes) |

Comments are `// line` and `/* block */`.

## Values and types

| Literal | Forms | Type |
|---|---|---|
| integer | `42`, `0x2A`, `1_000` | `int` (64-bit) |
| float | `2.0`, `1e-3` | `float` (32-bit) |
| duration | `2s`, `500ms`, `1.5min` | `Duration`, stored in seconds |
| angle | `90deg`, `1.57rad` | `Angle`, stored in radians |
| frequency | `10hz`, `2.5hz` | a rate — only `[Rate(…)]`'s argument; refused as a value |
| text | `"text"` | `string` |
| interpolated text | `$"hp: {h.current} / {h.max}"` | `string` — see [Interpolation](#interpolation) |
| boolean, none | `true`, `false`, `null` | `bool`, `T?` |

| Type | Status |
|---|---|
| `int`, `float`, `bool`, `string`, `Entity` | runs |
| `Vec2`, `Vec3`, `Vec4`, `Quat`, `Color` | runs — built by calling `Vec3(1.0, 2.0, 3.0)`; components `.x .y .z .w` (`.r .g .b .a`) read-only |
| `Duration`, `Angle` | runs — distinct from `float` and from each other; `2s + 500ms` is `2.5s`, `2s + 90deg` is refused |
| `T?` | runs — holds a value or `null`; one declared without a value starts `null`; `a ?? b`, `?.`, `if (var x = opt)`, `while (var x = opt)` and `match` unwrap it |
| `T[]` | runs — `[1, 2, 3]`, `xs[i]`, `xs.Length`, `foreach`, `xs.Push(v)`, `xs.RemoveAt(i)`; one declared without a value starts `[]` |
| `Prefab`, `Sound`, `Material`, `Mesh`, `Texture`, `Scene` | runs — a reference to an asset of that kind, by its identity, never its path; no literal: a field is `Prefab?`, chosen in the inspector; `==` / `!=` between two of one kind and against `null`, nothing else; shown in `$"…"` as `Asset#` and the first eight hex digits of its identity |
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
has none: write it `Entity?`, and it starts at `null`. Nor has an asset: write
`Prefab? bullet;` — or `Sound?`, `Material?`, … — and choose it in the
inspector's asset slot; it starts at `null`. The six kinds' names belong to the
language: `struct Prefab { … }` is refused. A state's data follows the
same rule; a value computed on entering a state is a parameter of `become`.

## Field attributes

```text
component Turret {
    [Header("Combat")]
    [Range(0, 100)] [Tooltip("How hard it hits")]
    int power = 40;
    [Range(0s, 5s)] Duration cooldown = 2.5s;
}
```

How the inspector shows a field of a `component`, a `behavior` or a `system`.
They are the editor's alone: nothing a script runs reads them, and a range
bounds the inspector, **never what a script writes**.

| Attribute | Means |
|---|---|
| `[Range(min, max)]` | an edit in the inspector stays between the bounds, both included; a gauge under the value shows where it sits. Its bounds are of the field's kind: integers on an `int`, numbers on a `float`, durations on a `Duration` (`[Range(0s, 5s)]`), angles on an `Angle` (`[Range(0deg, 90deg)]`) — on an optional or an array of one, the element's, each item held to it |
| `[Tooltip("…")]` | what hovering the field says |
| `[Header("…")]` | a section title above the field |

Each at most once, in any order. A value a script or a file wrote outside the
range is shown as it is, in amber, and changes only when someone edits it.
Editing an attribute applies at the next hot reload and never touches a row.

| Refused | Why |
|---|---|
| any other attribute on a field | not an attribute of a field |
| `[Range]` on a `bool`, a `string`, an engine value, an entity or a struct | applies to a number, a duration or an angle |
| bounds not of the field's kind, or not two | `[Range(0.5, 2)]` on an `int`, `[Range(0, 5)]` on a `Duration` |
| `min` not below `max`, or not finite | the range is empty |
| a default outside the range — including the zero a field without one starts at, and any item of an array's | a new row starts at the default: `[Range(1, 10)] int level;` needs `= 1` |
| a blank `[Tooltip]` or `[Header]`, or not one text | takes one text |
| an attribute on a `struct`'s field, or a state's | only a component's, a behavior's and a system's fields show in the inspector |
| an attribute before a method, a handler, `every`, `after` or `state` | attributes inside a behavior apply to its fields |

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
| `void Update(float dt)` | every turn, last, with the time since its last turn — the frame's delta, or more when a rate or the fuel kept it out of some frames |
| `void OnSpawn()` | once per entity, ever — a save remembers it ran; a hot reload does not rerun it |
| `void OnLoad()` | after a game save restored the instance, before anything else; never on a scene load or Stop |
| `void OnDespawn()` | when the despawn is decided — by `this.Despawn()`, or noticed a frame later |
| `void OnResumeFailed(string member)` | an edit left nothing of a body part-way through; `member` is e.g. `"OnSpawn"`, `"Chase.Attack"`, `"__every(0.5)"` |
| `void FixedUpdate(float dt)` | once per fixed simulation step the frame took, `dt` the step — before events and `Update`; never `async`, and refused if it reaches an `async` member |

See [One turn](../concepts/scripting/turns.md#one-turn) for their order. A
[system](#systems) takes `Update`, `FixedUpdate`, `OnLoad` and `OnResumeFailed`;
it has no entity to call `OnSpawn` or `OnDespawn` for.

### `FixedUpdate`

A **fixed clock**, not a physics callback. The engine counts the ticks of the
game's fixed clock (1/60 s) each frame — the same whether physics runs, and
whatever step physics sub-steps at — and each turn calls `FixedUpdate` once
per tick, after its own timers and before its events and `Update`:

- every call sees the frame's view of the world, plus the behavior's own writes;
  its writes reach physics next frame;
- a step the fuel does not reach is **owed** and runs first next turn, with the
  step it was taken at — at most five,
  saved with the instance; a fault drops them;
- paused, the game's clock stands still: no step, while `Update(0)` still runs;
- a state's `FixedUpdate` is used while in it, like `Update`.

## States

```text
behavior Guard {
    state Patrol {
        on Spotted(Entity foe) { become Chase(foe); }
    }
    state Chase(Entity foe) {
        int missed = 0;
        every 1s { foe.Raise(Damaged(5)); }
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

`await` itself is refused in `Update`, `on` handlers, `every` and `after`. It
takes a `Duration` (or a `float` of seconds); anything else is refused —
awaiting an event is not in the language yet.

## Expressions

| Expression | Status |
|---|---|
| `* / %`, `+ -`, `< <= > >=`, `== !=`, `&&`, `\|\|`, `!`, unary `-` | runs — C# precedence; `&&`/`\|\|` short-circuit; `int / int` truncates |
| `c ? a : b` | runs |
| `"a" + "b"`, `s += "b"`, `==` on text | runs — text `+` text only: `"hp " + health` is refused, offering `$"hp {health}"` |
| `$"…{x}…"` | runs — see [Interpolation](#interpolation) |
| `(float)x`, `(int)x` | runs — converts: `(float)5` is `5.0`, `(int)2.75` is `2`, `(int)-2.75` is `-2` (toward zero); `(int)` of an infinity, a NaN or a value past `int`'s range faults (`InvalidCast`) rather than saturating |
| `this` | runs — the behavior's entity; not in free functions |
| `Name(args)` | runs — functions and natives by bare name; arguments by position, then by name — see [Calls](#calls) |
| `obj.Method()` | checks |
| `a ?? b` | runs — `a` unless it is `null`; `b` is evaluated only then. A `float?` falls back to a float even when `b` is written as an int |
| `[a, b]`, `a[i]`, `a.Length`, `a?.b`, `Name { field: v }` | runs — see [Arrays and structs](#arrays-and-structs) |
| `new T(…)` | refused — not supported yet; build an engine type with its function, `Vec3(1.0, 2.0, 3.0)` |
| `entity.Field` | refused — an entity has no fields: a behavior's own fields are named directly, a component is read with `Get` |

## Calls

```text
fn float Move(float distance, float speed = 1.0, int turns = 0) { … }

Move(3.0);                      // speed 1.0, turns 0
Move(3.0, turns: 2);            // speed 1.0
Move(speed: 2.0, distance: 3.0) // any order
```

Arguments are positional first, then named, in any order. A parameter takes a
**constant default** — the rule a field's default follows, an engine value of
numbers included — and one without a default never follows one with. The
arguments run **in the order written**, whatever order the parameters are
declared in: `F(b: Mark(2), a: Mark(1))` marks 2 then 1.

| Takes names | By |
|---|---|
| a function, a behavior's or a state's method | its parameters |
| an engine function | its parameters, as [listed](#engine-functions): `Clamp(value: v, low: 0.0, high: 1.0)` |
| an engine constructor | its components: `Vec3(z: 1.0, x: 0.0, y: 0.0)`, `Color(r:, g:, b:, a:)` |
| an event's payload | its declaration: `this.Raise(Hit(amount: 3, source: this))` |
| `become` | the state's parameters: `become Chase(prey: e)` |

`Push`, `RemoveAt`, `Set`, `Add`, `Remove`, `Get`, `Has`, `Raise`'s argument
itself, `Spawn` and an attribute take their arguments in order, unnamed.

| Refused | Why |
|---|---|
| `F(speed: 2.0, 3.0)` | a positional argument cannot follow a named one |
| `F(1, x: 2)` | `x` is given twice |
| `F(sped: 2.0)` | `F` has no parameter `sped` — its parameters listed, the near one suggested |
| `F(turns: 2)` | `F` needs `distance` |
| `fn F(int x = Base())` | a parameter's default must be a constant |
| `fn F(int x = 1, int y)` | `y` needs a default: it follows `x`, which has one |
| a default on `Update`, a handler, an event, a state or an operator | the engine, a raise or `become` gives every value |
| `become Chase(1, 2)` to `state Chase(int prey)` | `become` is checked against the state's parameters |

## Interpolation

```text
Log($"{name} has {h.current}/{h.max} hp, cooldown {wait}");
// "Guard has 40/100 hp, cooldown 2.5s"
```

`$"…"` is a `string`. Each `{…}` holds one expression, shown by its type:

| Type | Shown as |
|---|---|
| `int` | `42` |
| `float` | its shortest form keeping the point: `2.0`, `0.1`, `1e-7`, `inf`, `NaN` |
| `Duration` | `2.5s` |
| `Angle` | in degrees, to a thousandth: `90deg` |
| `bool`, an empty optional | `true`, `null` |
| `Vec2`, `Vec3`, `Vec4`, `Quat` | `(1.0, 2.0, 3.0)` |
| `Color` | `rgba(1.0, 0.5, 0.0, 1.0)` |
| `Entity` | `Entity#12`, `Entity#12v3` past its first generation |
| `string` | as it is |

`{{` and `}}` write a brace. A hole may hold a call, a field, another string —
another `$"…"` too. A ternary in a hole is written in parentheses,
`{(alive ? "up" : "down")}`. The holes run left to right, and the text is built
once: one allocation, however many holes.

| Refused | Why |
|---|---|
| an array, a struct or a `Map` in a hole | `$"…"` shows numbers, text, flags, engine values and entities — show a part, `{xs.Length}` |
| `{}`, a lone `}` | an empty hole shows nothing; a `}` is written `}}` |
| `{a ? b : c}` | write `(a ? b : c)` |
| a call that returns nothing | the hole has nothing to show |

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
| Maths | `Abs(x)`, `Floor(x)`, `Ceil(x)`, `Round(x)`, `Sign(x)`, `Sqrt(x)` (faults below zero) `→ float`; `Min(a, b)`, `Max(a, b)`; `Clamp(value, low, high)` (faults if `low > high`); `Lerp(from, to, t)` (unclamped) |
| Text | `Log(message)`, `Warn(message)`, `Error(message)`; `Length(text) → int` |
| World — queued, applied at the frame's end | `Position() → Vec3` (own entity, as the frame began); called on an entity `e`: `e.Translate(delta)`, `e.SetPosition(value)`, `e.SetRotation(value)`, `e.SetScale(value)`, `e.Despawn()`, `e.SetParent(parent)`, `e.Detach()`, `e.SetMaterial(m)`; `Spawn(…)` and components — see [Writing components](#writing-components) |
| Input | `Pressed(action) → bool`, `JustPressed(action) → bool` |
| Constructors | `Vec2(x, y)`, `Vec3(x, y, z)`, `Vec4(x, y, z, w)`, `Quat(x, y, z, w)`, `Color(r, g, b, a)` |

The names in parentheses are the parameters' — what a named argument names.

A world write is a queued command: `Position()` does not see a `SetPosition` until
the next frame.

**An operation on an entity is called on it.** Every engine function whose first
parameter is an `Entity` is written `e.Name(…)` — `this.SetPosition(v)`,
`enemy.Despawn()`, `target.Raise(Hit(3))`. The free form `SetPosition(this, v)` is
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

behavior Turret {
    Prefab? shell;
    Material? hot;
    every 1s {
        var s = Spawn(shell, RigidBody { mass: 2.0 }, at: Position() + Vec3(0.0, 1.0, 0.0));
        s.SetMaterial(hot);
    }
}
```

| Write | Does | Refused |
|---|---|---|
| `e.Set(C { field: v, … })` | writes the fields named; the others keep their values | at the frame's end, when `e` has no `C` |
| `e.Add(C { field: v, … })` | attaches `C` with its defaults, then writes the fields named | at the frame's end, when `e` already has a `C` |
| `e.Remove(C)` | detaches `C` | at the frame's end, when `e` has no `C` |
| `Spawn(Vec3 position, C1 { … }, …) → Entity` | a new entity at `position`, upright, carrying each component | the whole spawn, when one component refuses its value — no entity is left behind |
| `Spawn(prefab, C1 { … }, …, at: p, facing: q) → Entity` | a linked instance of `prefab`, its root at `p`; `facing:` turns it, else it keeps the prefab's rotation and scale; each component given merges onto the root's, or is added | the whole instance, when a component refuses its value; a `null` prefab, or one the project does not hold, faults the turn |
| `e.SetMaterial(m)` | gives `e` the material `m`, loaded before it is drawn | a `null` `m` faults the turn |

- The components a script writes are those `engine/components.erg` declares with
  `component` — the ones an author may add in the editor — and the project's own
  ([Declaring components](#declaring-components)). `Transform` is placed with
  `SetPosition`, `Translate`, `SetRotation` and `SetScale`; a component the engine
  computes (`Derived`, `Runtime`) or a tool sets (`Parent`) is a `struct` there — a
  value a script can build, not write.
- A component literal is a *patch*: it appears only as the argument of `Set`, `Add`
  or `Spawn`, and its fields are evaluated in the order written. A component is not
  a type — `RigidBody r;` is refused.
- Each write costs `1 +` the number of fields it names, a spawn `1 +` the number
  of components it gives.
- **`Spawn` returns the entity.** Its id is reserved in the turn: the entity is
  not alive until the frame's end, but what the turn queues after the spawn names
  it, and `b.Get(C)` / `b.Has(C)` read what it was spawned with and written since.
  A prefab's own components are read from the next frame. Kept in a field, the
  entity is the same one next frame; a refused spawn leaves an id no entity ever
  holds. See [Scripting: an entity named before it exists](../concepts/scripting/writing.md#an-entity-named-before-it-exists).
- In the prefab form the prefab comes first, unnamed, then the components, then
  `at:` (required, a `Vec3`) and `facing:` (a `Quat`); any other label is refused.
  For a strict add or replace, use the verbs on the entity it returns:
  `s.Add(C { … })` refuses a component the prefab has, `s.Remove(C)` then
  `s.Add(C { … })` replaces it whole.
- An instance spawned in play is saved in a game save flattened, with its link:
  it reloads as the prefab was when it was saved. How a save keeps such instances
  is being redesigned.
- **A turn that faults applies nothing** of what it queued in that frame — its
  writes, spawns, removals and events — and the ids it reserved go back unspent.
- `Set`, `Add`, `Remove`, `Get`, `Has`, `Spawn` and `SetMaterial` are reserved: no
  function or method may take one of these names.

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

## Scheduling

```text
[Critical] behavior Player { … }          // served first
[Budget(0.2ms)] behavior Crowd { … }      // never more than 0.2 ms a frame
[Rate(10hz)] behavior Patrol { … }        // at most ten turns a second
```

How the script lane schedules a behavior or a system when the frame's budget
will not cover every one. None of them changes what a turn does — only whether
it runs this frame, and with how much of the fuel; the run order stays the
module's.

| Attribute | Means |
|---|---|
| `[Critical]` | served first: a turn for each of its instances due this frame, at what a turn of it costs, before the others share the rest — and what nobody else takes. A budget too short goes to the critical behaviors alone; it is never exceeded |
| `[Budget(d)]` | one positive duration: the most it spends a frame; what it does not take goes to the others |
| `[Rate(f)]` | one positive frequency: at most `f` turns a second. Its instances spread over the period, each a turn once a period; a frame it skips keeps its mail and is not a deferral. Refused with a `FixedUpdate` |

Each at most once. **No time is lost** to a frame an instance does not run — for
its rate or for fuel: its next turn is handed all the time since its last, its
`Update`'s `dt`, its timers and its waits.

## Events

```text
event Damaged(int amount, Entity source);

behavior Guard {
    int health = 100;
    on Damaged(int amount, Entity source) { health -= amount; }
}

behavior Spike {
    on Touched(Entity other) { other.Raise(Damaged(30, this)); }
}
```

An event is a **declaration**: its name is its identity, its parameters its
signature, and both ends are checked against it at compile time.

| Written | Rule |
|---|---|
| `event Name(T a, …);` | a top-level item, visible in its module and every module importing it. `event Name;` and `event Name();` carry nothing. Its name is global — refused when another declaration, an engine function or type, a lifecycle hook (`Update`, `OnSpawn`, …), an operation (`Set`, `Get`, `Spawn`, `Raise`, …) or a `__` name has it |
| a payload | `bool`, `int`, `float`, `string`, `Vec2`, `Vec3`, `Vec4`, `Quat`, `Color`, `Entity`, `Duration`, `Angle`. An optional, an array, a struct, a map or a component is refused at the declaration |
| `target.Raise(Name(args))` | `target` any `Entity`, in a behavior, a function or a system. The arguments are checked against the declaration — count and types; an `int` is taken where it says `float`, never where it says `Duration` or `Angle` |
| `Raise("Name", …)` | refused — the message gives the `event` to declare and the raise to write |
| `Name(args)` elsewhere | refused: an event is raised, not stored |
| `on Name(T a, …) { … }` | must name an event in scope — a typo is a compile error. It takes **all** of the payload, its types in order (names free), **or none**: `on Name { … }`, `on Name() { … }`, `on Name => …;` |
| a method or function named like an event | refused — `Name()` would read as the event's construction |

At run time:

- A raise is delivered **next frame**, to the current state's handler, then the
  behavior's; a behavior with neither is not told; a despawned target hears
  nothing.
- Only a declared event is delivered: one the engine sends named like a method
  or a hook runs nothing.
- The payload is checked against the event at delivery. One that does not fit —
  sent by the engine, or queued before a reload changed the event — is
  **dropped and reported** (a warning in the editor's Problems tab, one line per
  event); the receiver keeps running.
- The engine raises `Touched(Entity other)` and `Separated(Entity other)` from
  collisions. They are declared by the engine, served as `engine/events.erg`
  and in scope in every module without an import; a script may not declare
  either name.
- A play session starting over in the editor drops the last session's
  undelivered events.

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
([its one entity](../concepts/scripting/behaviors-and-systems.md#its-one-entity)). Its members are a
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
| `[Domain(…)]`, an unknown attribute | not an attribute of a system | its fields live in the `Script` domain; a system takes `[Critical]`, `[Budget(…)]`, `[Rate(…)]` |
| a name another declaration, an engine component or its `formerly` has | declared twice, or an engine component | a system is a component: one namespace for all |

A system's component is not one a script reaches: `e.Get(Spawner)`,
`e.Set(Spawner { … })`, `Add`, `Remove`, `Has` and a query binding `Spawner`
are refused as not a component a script writes or binds — another holder would
be a second instance.

`system` and `ref` are keywords. `with` and `without` are words only after a
query's parentheses; anywhere else they are ordinary names — as `on` is one
only where a handler starts and `event` only where a declaration does.

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
  [Queries and sweeps](../concepts/scripting/queries.md#queries-and-sweeps).
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
closures, reading `Transform` with `Get` (use `Position()`), reordering systems
(`[Before]` / `[After]`), change filters in queries, systems running in parallel,
a format in an interpolation's hole (`{x:F2}`), named arguments to the position
form of `Spawn`, playing a `Sound` (it comes with the audio work), loading a
`Scene`, spawning a `Mesh`.

## See also

- [Scripting](../concepts/scripting/index.md) — turns, safepoints, saves, resuming
  after edits.
- [Glossary](./glossary.md) — asset reference, reserved entity, safepoint, site, resume tier, system, query, sweep,
  watchdog, declared component, event, field attributes, named argument,
  interpolation, ScriptProblems.
- [Editor — Scripts](./editor.md#07--scripts) — scripts while editing, the
  Problems tab, declared components in the Inspector.
