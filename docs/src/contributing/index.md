# Contributing

This section is for people working **on the engine itself** — fixing bugs in a
subsystem, adding a lane or an agent, extending the ECS, improving the renderer.
If instead you want to **build a game on top of Khora**, you want the
[tutorials](../tutorials/your-first-game.md) and the game-side
[how-to guides](../how-to/index.md) — not this section.

Contributing to an engine with a strong architecture (SAA, the CLAD descent,
GORNA) means there are conventions you have to internalise before your change
will pass review and CI. These pages give you the fastest path to productive,
mergeable work.

## The path

Follow these in order on your first contribution; come back to any of them as a
reference later.

1. **[Set up your environment](./setup.md)** — clone, build, run the sandbox and
   the editor, serve the docs. The ordered getting-started that gets a working
   tree in front of you.
2. **[Take the architecture tour](./architecture-tour.md)** — a guided reading
   order through the [Concepts](../concepts/index.md) so you understand the
   internals fast, in the right sequence, before you touch code.
3. **[Learn the conventions](./conventions.md)** — the hard rules and coding
   conventions every change must follow (math types, logging, error handling,
   components, agents, lanes, shaders, the dependency direction).
4. **[Know the workflow](./workflow.md)** — the branch/PR flow and the CI gates
   your change must pass (format, clippy, the cross-platform test matrix,
   doctests, supply-chain, MSRV).

## When you're ready to write code

Once you have the lay of the land, the hands-on material lives in two places:

- The **[Extending the engine](../tutorials/extending-the-engine.md)** tutorial —
  a guided, end-to-end walk through adding a new piece to the engine.
- The engine-side **[how-to recipes](../how-to/index.md)** — task-sized guides
  for adding a component, a lane, an agent, a shader, an asset decoder, or a
  flow.

Read those for the *how*; this section gives you the *setup*, the *map*, and the
*rules* that make them stick.
