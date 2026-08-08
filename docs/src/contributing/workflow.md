# Contribution workflow

This page covers how a change goes from your working tree to `main`: the
branch/PR flow, the gates CI enforces, and how releases are cut.

## Talk first

Given the architectural nature of the current work, **open an issue or a
discussion before writing a non-trivial change** so it lines up with the roadmap
and the architecture. Bug reports and feature requests go through the
[issue templates](https://github.com/eraflo/KhoraEngine/issues); broader ideas go
to [GitHub Discussions](https://github.com/eraflo/KhoraEngine/discussions).

## Branch and PR flow

- Development happens on **`dev`**; stable releases land on **`main`**.
- Work on a branch, then open a **pull request targeting `main`** and link it to
  the relevant issue.
- Before opening the PR:
  1. Run **`cargo xtask all`** locally (format + clippy + build + test + doc) so
     your change is already green against the gates below.
  2. Update the docs alongside the code — this book for concepts/narrative,
     rustdoc comments for API surface. A public-API change updates its mdBook
     chapter in the same PR.

A `secret-scan` pre-commit hook (installed by the AI tooling) blocks commits that
contain secrets. Commit messages follow imperative mood with an optional
conventional prefix (`feat:`, `fix:`, `refacto:`, `docs:`) — the prefix matters,
because releases are derived from it (see [Releases](#releases)).

## CI gates

Every push to `main` and every pull request targeting `main` runs the **Rust CI**
workflow. All of the following must pass before a PR can merge (an `All Checks
Pass` job aggregates them):

| Gate | Command | Notes |
|---|---|---|
| **Format** | `cargo fmt --all -- --check` | Must report no changes. |
| **Lint** | `cargo clippy --workspace --all-targets --all-features -- -D warnings` | Warnings are errors. |
| **Test — Linux** | `cargo nextest run --workspace --all-targets --all-features` | Runs on `ubuntu-latest`. |
| **Test — Windows** | same | Runs on `windows-latest`. |
| **Test — macOS** | same | Runs on `macos-latest`. |
| **Doctests** | `cargo test --workspace --doc --all-features --locked` | Separate job — `nextest` does **not** run doctests, so rustdoc `# Examples` are verified only here. |
| **Supply chain** | `cargo deny check` | Advisories, licenses, and duplicate-version policy from `deny.toml`. |
| **MSRV** | `cargo check --workspace --all-features --all-targets --locked` on **1.95** | Fails if a change uses a newer stdlib/language feature than the pinned MSRV. |

A **coverage** job also runs but is **report-only** — it produces an `lcov.info`
artifact, has no threshold, and never blocks the merge.

### Notes on specific gates

- **The test matrix runs on all three platforms.** A change that compiles and
  passes on your OS can still fail on another (path handling, backend
  availability) — write platform-portable code.
- **Doctests are their own gate.** Because `nextest` skips doctests, the only
  place your rustdoc examples are compiled and run is the doctests job. Keep
  `# Examples` blocks correct, or run them locally with
  `cargo test --workspace --doc`.
- **`cargo deny` and the bincode advisory.** The supply-chain gate fails on
  vulnerabilities. One advisory is deliberately ignored in `deny.toml`:
  **`RUSTSEC-2025-0141`**, which flags `bincode` as unmaintained — a notice, not a
  vulnerability, kept in the ignore list with its rationale documented inline. If
  you add a dependency that trips a *new* advisory, the gate fails until it is
  resolved or explicitly justified.
- **MSRV is 1.95.** This gate is a plain `cargo check` on the pinned toolchain. If
  your change needs a newer language or stdlib feature, the MSRV has to be bumped
  deliberately — it is not raised implicitly by a PR.

## Releases

Releases are automated. On a push to `main`, a **semantic-release** workflow
analyses commit history, derives the next version from the conventional-commit
prefixes, updates the changelog, builds the release binaries (engine, hub,
runtime) for Windows, Linux, and macOS, and publishes them. This is why commit
message prefixes matter: a `feat:` and a `fix:` produce different version bumps.
Contributors don't cut releases by hand — writing correct commit messages is the
whole input.

---

That's the full loop: set up your tree, learn the map, follow the conventions,
pass the gates. Ready to build something? Start with the
[Extending the engine](../tutorials/extending-the-engine.md) tutorial or pick an
engine [how-to recipe](../how-to/index.md).
