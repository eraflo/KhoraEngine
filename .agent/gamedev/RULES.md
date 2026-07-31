# Khora SDK — Rules (gamedev profile)

Hard constraints for building a game with the Khora Engine. Read before writing game code.

- Document — Khora SDK Rules v1.0
- Status — Authoritative

---

## 1 — Must always

- Build your game against the **`khora-sdk`** public API and its `prelude` only.
- Implement `EngineApp + AgentProvider + PhaseProvider` for your game type; boot via `run_winit`.
- Spawn entities through `GameWorld` / `Vessel` and the spawn helpers — not by hand-poking storage.
- Math via `khora_sdk::prelude::math` (`Vec3`, `Quaternion`, `LinearRgba`) — never raw `glam`.
- Log via `log::{info,warn,error}` (set up `env_logger` in `main`) — never `println!`.
- Register the backends you need once in the `run_winit` bootstrap closure (render, physics, layout, text, audio).
- `cargo build` + `cargo run` your game after changes to confirm it works.

## 2 — Must never

- **Never depend on internal `khora-*` crates** (`khora-control`, `khora-agents`, `khora-lanes`,
  `khora-data`, `khora-infra`, …) directly. If the SDK doesn't expose what you need, ask — do not work
  around the façade. (`khora-sdk` re-exports the few internal types you legitimately need.)
- Never assume engine internals (the CLAD descent, GORNA negotiation, agents, lanes) — they run for you.
- Never `unwrap()` on fallible I/O (asset loading, file access, device open). Handle the `Result`.
- Never embed secrets/keys in a game build or commit them to git ([`security-privacy.md`](./security-privacy.md)).
- Never push to git or create PRs without explicit permission.

## 3 — Boundaries (do-not-touch)

- Don't edit the generated AI wrappers (`CLAUDE.md`, `AGENTS.md`, `GEMINI.md`, `.github/`, `.claude/`,
  `.cursor/`, `.gemini/`) — they regenerate from `.agent/gamedev/` via the installer. Edit the source here.
- Don't touch `target/` or vendored code.

## 4 — Permission model

- **No prompt:** read files, `cargo build`/`cargo run` your game, run the SDK examples.
- **Ask first:** installing dependencies, any `git` write op, deleting files, packaging/shipping a build.
- **Never without explicit permission:** push to git, create PRs.

## 5 — Design

- For any UI / HUD / menu / visual decision, use **`/impeccable`** (audit / critique / polish).

*End of rules.*
