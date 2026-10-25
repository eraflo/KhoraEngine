# Scripting

<p class="kp-lede">A <em>script</em> is a short text file that tells the things in your game what to do:
a door that opens when you come near, a guard who walks its round, a coin that
disappears when you pick it up. Khora's scripts are written in a language made
for games, called <strong>Ergon</strong>.</p>

You do not need to know how to program to start here. This page begins with the
picture, then has you write and run a first script in the editor. The pages
after it go deeper, one idea each, down to how the engine runs thousands of
scripts inside a frame.

## The picture: a flip-book

A game is a flip-book. About sixty times a second the engine draws a new page —
a **frame** — and the pages flipping by are what you see moving. On each page,
every scripted thing gets a short moment to look around and decide what to do:
its **turn**. Click through:

<figure class="kp-steps">
<div class="kp-figure-frame">

{{#include ../../images/scripting/flipbook.svg}}

</div>
<ol class="kp-steps-text">
<li><strong>A page.</strong> This is one frame of a game: a guard, a coin, the ground. The engine will draw about sixty of these every second.</li>
<li><strong>The guard's turn.</strong> The guard has a script, and its script says what to do on every page: take one step toward the coin.</li>
<li><strong>A note, not a move.</strong> The guard does not move itself. It hands the engine a note — "one step right" — and the engine does it once every script has had its turn.</li>
<li><strong>Page after page.</strong> Each new page, the script runs again. A step at a time, sixty times a second, the guard reaches the coin. That is all a game is: small decisions, many times a second.</li>
</ol>
</figure>

Three words carry the rest of this chapter:

| Word | In plain words |
|---|---|
| **entity** | one thing in the game — the guard, the coin, the camera, the light |
| **component** | one fact about an entity — where it is, how much health it has, which mesh it is drawn with. An entity is the sum of its components |
| **behavior** | a script attached to an entity, as one of its components — the guard's "walk toward the coin" |

## Try it: your first script

You need the editor open on a project. If you do not have one yet, [Start from
zero](../../tutorials/start-from-zero.md) takes you there from nothing.

1. **Put something in the scene.** In the **Scene Tree**, right-click the empty
   space and choose **Add → Cube**. A cube appears in the viewport.

2. **Make a script.** In the **Assets** panel, right-click an empty space and
   choose **New Script**. The panel opens the `scripts` folder — the one the
   engine reads scripts from — and shows a new tile with a name field under it.
   Type `Mover` and press **Enter**. The editor writes `scripts/Mover.erg`: a
   small script that already works, each part saying what it is for.

3. **Make it do something.** Double-click the *Mover.erg* tile: it opens in your
   computer's text editor. If the computer asks which app to use, pick Notepad
   (Windows) or TextEdit (macOS) — any text editor will do. Select everything
   in the file and replace it with:

   ```text
   behavior Mover {
       float speed = 1.0;

       void OnSpawn() {
           Log("the cube is alive");
       }

       void Update(float dt) {
           this.Translate(Vec3(speed * dt, 0.0, 0.0));
       }
   }
   ```

   Save the file. The editor reads it again the moment you save; if something is
   wrong, the **Problems** tab at the bottom says which line, and why.

4. **Give the cube the script.** Select the cube. In the **Inspector**, click
   **+ Add Component** and choose **Mover**. A card named *Mover* appears, with
   its `speed` field.

5. **Press Play.** The cube slides to the right, one unit per second, and the
   **Console** says *the cube is alive*. Change `speed` in the Inspector while it
   plays and watch it change. Press **Stop**: the cube goes back where it was —
   nothing a game does while you test it is kept.

What each line said:

| Line | Means |
|---|---|
| `behavior Mover { … }` | a kind of brain called *Mover* — the name you typed. Attached to an entity, it becomes one of its components |
| `float speed = 1.0;` | a **field**: a number the behavior keeps, starting at `1.0`. Fields show in the Inspector, so you can tune them without touching the code |
| `void OnSpawn() { … }` | runs once, the first time the behavior runs on an entity |
| `void Update(float dt) { … }` | runs on every frame. `dt` is the time since the last frame, in seconds — multiplying by it makes `speed` a speed *per second*, whatever the frame rate |
| `this.Translate(…)` | asks the engine to move this entity. Like the guard's note, it happens when every script has had its turn |

From here, the [Ergon reference](../../reference/ergon.md) lists everything the
language can say.

## If you know Unity or Godot

| You know | In Khora |
|---|---|
| a `MonoBehaviour` / a node's script | a `behavior`, attached as a component |
| `Start()` / `_ready()` | `OnSpawn()` |
| `Update()` / `_process(delta)` | `Update(float dt)` |
| `FixedUpdate()` / `_physics_process` | `FixedUpdate(float dt)` |
| `[SerializeField]` / `@export` | every field — the Inspector edits it |
| `GetComponent<T>()` / `get_node` | `e.Get(T)` — a copy; write with `e.Set(T { … })` |
| `Instantiate(prefab)` / `instantiate()` | `Spawn(prefab, at: p)` — returns the new entity |
| coroutines, `yield return new WaitForSeconds` / `await` timers | `async` members and `await 0.5s`; `every 2s { … }`, `after 1s { … }` |
| `SendMessage` / signals | `event`s, raised on an entity — heard on the next frame |
| a state machine you write by hand | `state` blocks and `become` |
| a script that writes the scene at once | a script that **asks**: every write is applied at the frame's end |

The last row is the one to remember. It is what lets the engine run your scripts
beside the renderer rather than after it, and stop one part-way through a
thought without breaking it — [Changing the world](./writing.md) shows how.

## Deeper, one idea at a time

Each page opens with the idea in plain words, then goes down to how it works,
what it guarantees and where it stops.

- [Behaviors and systems](./behaviors-and-systems.md) — one entity's logic, or
  one rule over many.
- [Turns and time](./turns.md) — what runs in a frame, in what order, and how a
  script can be stopped mid-thought and pick up where it left off.
- [Changing the world](./writing.md) — why a script asks instead of acting, and
  the entity it can name before it exists.
- [Queries and sweeps](./queries.md) — "every entity with health and no shield",
  visited one by one.
- [Events](./events.md) — one behavior telling another, always next frame.
- [Saves and live edits](./saves-and-edits.md) — a game saved mid-sentence, and
  code edited while it runs.
- [Under the hood](./under-the-hood.md) — why Khora has a language of its own,
  and who owns what inside the engine.
