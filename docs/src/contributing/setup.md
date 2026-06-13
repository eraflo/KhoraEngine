# Setting up a dev environment

This is the ordered getting-started for working **on** the engine. By the end you
will have a clean build, a green test run, and the sandbox, editor, and docs all
running locally.

## Prerequisites

- **Rust toolchain.** The minimum supported Rust version (MSRV) is **1.91**. CI
  enforces it with a dedicated `cargo check` on 1.91, so anything newer than the
  stdlib/language surface of 1.91 will fail the build. Install with
  [`rustup`](https://rustup.rs); `stable` is fine for day-to-day work as long as
  your change still compiles on 1.91.
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

The primary check is the whole-workspace test run:

```bash
cargo test --workspace
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

- **`cargo xtask`** — the workspace build-automation entry point. `cargo xtask all`
  runs the full local gate (build, test, check, format, clippy) — run it before
  opening a pull request. Other subcommands cover assets (`cargo xtask assets pack`)
  and regenerating the AI doc wrappers.
- **`cargo hub-dev`** — the contributor dev loop described above (build editor +
  runtime, launch the hub).
- **The hub** — the project manager / engine launcher; `cargo hub-dev` builds and
  runs it for you.

## Keeping `target/` in check

`[profile.dev]` already uses `debug = "line-tables-only"` workspace-wide, which
strips most debuginfo while keeping panics and `RUST_BACKTRACE=1` useful. When the
build directory still grows too large, `cargo clean` resets it; for a lighter
touch, `cargo install cargo-cache` then `cargo cache --autoclean` drops stale
incremental and registry artifacts without forcing a full rebuild. If you need a
real debugger and line tables aren't enough, override `debug = 2` in a local
`Cargo.toml` patch kept out of git.

---

Tree builds, tests pass, sandbox and editor run? Good. Next, take the
[architecture tour](./architecture-tour.md) before you start changing code.
