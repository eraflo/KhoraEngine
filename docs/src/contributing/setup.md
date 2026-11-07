# Setting up a dev environment

This is the ordered getting-started for working **on** the engine. By the end you
will have a clean build, a green test run, and the sandbox, editor, and docs all
running locally.

## Prerequisites

- **Rust toolchain.** The minimum supported Rust version (MSRV) is **1.95**. CI
  enforces it with a dedicated `cargo check` on 1.95, so anything newer than the
  stdlib/language surface of 1.95 will fail the build. Install with
  [`rustup`](https://rustup.rs); `stable` is fine for day-to-day work as long as
  your change still compiles on 1.95.
- **System libraries (Linux).** The audio, windowing, and UI backends need native
  dev packages. On Debian/Ubuntu:

  ```bash
  sudo apt-get update
  sudo apt-get install -y libasound2-dev pkg-config libgtk-3-dev \
      libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev
  ```

  Windows and macOS need no extra system packages for a default build.

## 1. Clone and build

```bash
git clone https://github.com/eraflo/KhoraEngine.git
cd KhoraEngine
cargo build --workspace
```

The first build compiles the full graphics/physics/audio stack and takes a while;
later incremental builds are fast.

## 2. Run the tests

The primary check is the gate — what CI runs, with CI's flags. It needs two
cargo tools, installed once:

```bash
cargo install cargo-nextest cargo-hakari --locked
```

```bash
cargo gate
```

This must be green before you start changing things — it tells you the tree is
healthy on your machine. (CI runs the same suite cross-platform via
`cargo nextest`; see [Workflow](./workflow.md).)

## 3. Run the sandbox

The sandbox is a small example game that exercises the engine directly through
the SDK — the quickest way to see a frame loop running:

```bash
cargo run -p sandbox
```

Watch the console: the frame loop should be clean, with no GPU validation errors.

## 4. Run the editor

```bash
cargo run -p khora-editor
```

For the full author-time loop, the editor needs the **runtime** binary on disk so
its "Build Game" feature can stamp it onto a project. The convenience alias builds
editor and runtime in debug and then launches the **hub** (the project manager /
engine launcher) in release:

```bash
cargo hub-dev
```

That alias is equivalent to:

```bash
cargo build -p khora-editor -p khora-runtime
cargo run   -p khora-hub --release
```

Use `cargo hub-dev` rather than typing the chain by hand — without
`khora-runtime` present, the editor's "Build Game" step fails with
"khora-runtime binary not found".

## 5. Serve the documentation

This book is built with [`mdBook`](https://rust-lang.github.io/mdBook/). Install
it once, then serve with live reload from the repo root:

```bash
cargo install mdbook
mdbook serve docs --open
```

## Tooling you'll use

- **`cargo xtask`** — the workspace build-automation entry point. `cargo gate`
  (`cargo xtask gate`) runs the full local gate (format, clippy, tests, doc
  tests) — run it before opening a pull request; `cargo t -p <crate> [filter]`
  runs a subset of the tests while you iterate. Other subcommands cover assets (`cargo xtask assets pack`)
  and regenerating the AI doc wrappers.
- **`cargo hub-dev`** — the contributor dev loop described above (build editor +
  runtime, launch the hub).
- **The hub** — the project manager / engine launcher; `cargo hub-dev` builds and
  runs it for you.

## Keeping `target/` in check

What makes `target/` large is not the number of lines but the number of times the
whole engine is built and linked. Three settings keep that down:

- **No debuginfo in dev builds** (`[profile.dev] debug = 0`, dependencies too).
  The workspace links some fifty binaries, each with the whole engine, and on
  Windows each link wrote a PDB of over 100 MB. A panic still names its file and
  line (`Location` is compiled in); a backtrace names functions without lines.
  To debug, build with `CARGO_PROFILE_DEV_DEBUG=line-tables-only` (or `2`).
- **One feature set.** The workspace crates declare no optional feature, and
  the third-party dependencies build with one feature set each, unified by the
  `workspace-hack` crate that `cargo hakari` generates. So
  `cargo run -p khora-editor`, `cargo t -p <crate>` and `cargo gate` build the
  same engine, not one each. After adding or changing a dependency, run
  `cargo hakari generate` and `cargo hakari manage-deps`; the gate fails until
  you do.
- **No test build for a target without tests.** A target that holds no test
  sets `test = false` (`sandbox`, `khora-runtime`). The gate runs cargo's
  default test targets, which leave those out, then the benches as smoke tests
  (`--bench '*'`; `--all-targets` would build the left-out targets anyway, as
  an explicit selection overrides `test = false`). An `xtask` check fails if a test is written in a target
  marked `test = false`.

The rest is pruned at the start of each agent session by
`khora-ai.mjs sweep` (see `.agent/engine/knowledge/context.md`): incremental
caches unused for 3 days, stale side dirs (`miri`, a nightly clippy), then,
past 20 GiB, every incremental cache before the profiles themselves.
`cargo clean` resets everything.

---

Tree builds, tests pass, sandbox and editor run? Good. Next, take the
[architecture tour](./architecture-tour.md) before you start changing code.
