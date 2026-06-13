# Reference

Information-oriented lookup. These pages are curated maps and tables — accurate,
neutral, structured for finding a fact fast. They do not explain *why* (see
[Concepts](../concepts/index.md)) or walk through a task (see
[How-to guides](../how-to/index.md)).

> **The exhaustive API lives in rustdoc, not here.** Every public type, trait,
> function, and method is documented in the generated rustdoc. The book links to
> it rather than restating it. Start at the published reference:
> [eraflo.github.io/KhoraEngine/api](https://eraflo.github.io/KhoraEngine/api/index.html).

## Pages

| Page | What you look up here |
|---|---|
| [API reference](./api.md) | The gateway to the published rustdoc — per-crate links and how to build it locally. |
| [SDK surface](./sdk.md) | The public `khora-sdk` surface: app traits, `run_winit`, `GameWorld`, `Vessel`, the prelude. |
| [Crate map](./crates.md) | All workspace crates, their one-line roles, the dependency direction, and a "where things live" table. |
| [File formats](./formats.md) | The `.kscene` scene file, the `.pack` archive, and the `.kmat` material format — structure and facts. |
| [Project structure](./project-structure.md) | The Khora project folder layout, `project.json` schema, asset extensions, and the three code tiers. |
| [Glossary](./glossary.md) | The vocabulary index — SAA, CLAD, GORNA, AGDF, CRPECS, Lane, Flow, and the rest. |

---

*Need the rationale? See [Concepts](../concepts/index.md). Need the steps? See
[How-to guides](../how-to/index.md). Need every signature? See the
[rustdoc](https://eraflo.github.io/KhoraEngine/api/index.html).*
