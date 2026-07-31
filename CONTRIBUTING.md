# Contributing to KhoraEngine

First off, thank you for considering contributing to KhoraEngine! It's an ambitious project, and every contribution, from a bug report to a documentation fix, is greatly appreciated.

KhoraEngine is currently in its early stages of development. We are primarily focused on building the core Symbiotic Adaptive Architecture (SAA) and its foundational components.

## Development Prerequisites

Before contributing, please ensure you have the required tooling installed.

### Documentation (`mdBook`)

Our narrative documentation is built using `mdBook`. To preview your documentation changes locally, you will need to install it:

```bash
# Install mdBook via cargo
cargo install mdbook
```

Once installed, you can serve the book locally from the project root:
```bash
# This command builds the book, starts a local web server, and opens it in your browser.
mdbook serve docs --open
```

## Dev workflow shortcuts

The hub launches the editor, the editor stamps the runtime onto your project's pack — three binaries that have to be present on disk. A fresh clone has none of them built. Use the alias below instead of typing the chain manually:

```bash
# (1) Build editor + runtime in debug, (2) launch the hub in release.
# Both editor and runtime must be in target/debug/ for the hub's
# dev_engine() to find them.
cargo hub-dev
```

Equivalent to:

```bash
cargo build -p khora-editor -p khora-runtime
cargo run   -p khora-hub --release
```

Without `khora-runtime` on disk, the editor's "Build Game" feature can't stamp it onto your project's data-only export and fails with "khora-runtime binary not found". `cargo hub-dev` builds it as part of the loop so this never happens.

### Keeping `target/` from exploding

Two practical knobs:

- **Already applied workspace-wide** in [`Cargo.toml`](./Cargo.toml): `[profile.dev] debug = "line-tables-only"` strips the bulk of debuginfo while keeping panics + `RUST_BACKTRACE=1` useful. Saves roughly 30–50% on every dev build.
- **Periodic full clean** when target/ still grows out of hand: `cargo clean` wipes everything and forces a full rebuild on the next invocation. For a finer-grained cleanup, install `cargo-cache` (`cargo install cargo-cache`) and run `cargo cache --autoclean` to drop old incremental and registry artifacts without nuking the active build.

If you need a real debugger session and `line-tables-only` isn't enough, override locally with a `Cargo.toml` patch outside of git:

```toml
[profile.dev]
debug = 2  # full debuginfo, larger target/
```

## How Can I Contribute?

*   **Join the Discussion:** Share your thoughts, ideas, and feedback on our [GitHub Discussions page](https://github.com/eraflo/KhoraEngine/discussions).
*   **Report Bugs:** If you experiment with the engine and find bugs, please open an [Issue](https://github.com/eraflo/KhoraEngine/issues) using the "Bug Report" template, providing detailed steps to reproduce.
*   **Suggest Features:** Have ideas for features that would complement the SAA vision? Propose them as an [Issue](https://github.com/eraflo/KhoraEngine/issues) using the "Feature Request" template.
*   **Improve Documentation:** If you see areas for improvement or clarification in our documentation book, pull requests are welcome!

## Code Contributions

Direct code contributions are welcome, but given the architectural nature of the current work, please start by opening an Issue or a Discussion to talk about your proposed changes with the maintainer (@eraflo) first. This ensures that your work aligns with the project's roadmap and architectural principles.

### Pull Request Process

1.  Ensure any new code adheres to our coding standards by running `cargo xtask all`.
2.  Update the documentation (`mdBook` for concepts, `rustdoc` comments for API) with any relevant changes.
3.  Open a pull request and link it to the relevant issue.

## Code of Conduct

By participating in this project, you are expected to uphold our [Code of Conduct](CODE_OF_CONDUCT.md).

We look forward to building this innovative engine with you!