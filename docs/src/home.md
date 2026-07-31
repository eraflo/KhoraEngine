# Khora Engine

**An engine that thinks.**

Khora is an experimental real-time game engine, written in Rust, built on a
**Symbiotic Adaptive Architecture**: every major subsystem is an agent that
knows its own cost, weighs its options, and negotiates for a slice of the frame
budget — each tick. A central observer watches thermal headroom, frame-time
stutter, battery, and GPU pressure, and trades budgets with the agents through a
protocol called **GORNA**. The agents adapt; the work continues.

Most engines decide at compile time. Khora decides at runtime, every tick.

---

## Choose your path

This documentation is organized around what you came to do. Pick a starting
point — each path is a guided thread, not a pile of chapters.

### 🎮 I want to build a game

You want a window, a camera, and something moving on screen — fast. The engine
handles the performance problem; you focus on the creative one.

**Start with [Your first game](./tutorials/your-first-game.md).** It takes you
from a clean clone to a running scene you can fly around in, step by step. From
there, the [How-to guides](./how-to/index.md) are task-sized recipes ("load a
mesh", "play a 3D sound", "save a scene"), and the [SDK overview](./reference/sdk.md)
maps the public surface.

### 🧠 I want to understand the engine

You want the ideas: why an engine would negotiate with itself, how the layers
fit, what CRPECS and GORNA and AGDF actually are.

**Start with [The big idea](./concepts/saa.md).** The [Concepts](./concepts/saa.md)
section is the "why" — the philosophy, the architecture, and one explanation page
per subsystem. Read it in order, or jump to the [Glossary](./reference/glossary.md)
when a term is unfamiliar.

### 🔧 I want to contribute

You want to extend the engine — a custom agent, a new lane, a backend — or work
on the internals.

**Start with [Get set up](./contributing/setup.md)**, then take the
[Architecture tour](./contributing/architecture-tour.md) for a guided reading
order. [Extending the engine](./tutorials/extending-the-engine.md) is a worked
tutorial; the [conventions and rules](./contributing/conventions.md) are the
constraints we hold to.

---

## In a hurry?

```bash
git clone https://github.com/eraflo/KhoraEngine.git
cd KhoraEngine
cargo build
cargo run -p sandbox        # the demo — a scene you can fly around
```

Then open [Your first game](./tutorials/your-first-game.md) to build your own.

---

## Status

Khora is **experimental**. The foundational architecture, the CRPECS ECS, the
GORNA negotiation loop, six intelligent agents, an editor with play mode, and a
large workspace test suite are operational. The SDK surface is intentionally
narrow and grows as the engine matures. The [Roadmap](./roadmap.md) lays
out the multi-year path; the [Open questions](./open_questions.md)
chapter is honest about what is still undecided.

When the engine changes, this book changes in the same commit.

*An engine that thinks. A book that says so plainly.*
