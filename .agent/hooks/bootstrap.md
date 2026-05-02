# Bootstrap

On session start:

1. Confirm the workspace root is `KhoraEngine/` and contains `Cargo.toml` with `[workspace]`.
2. Check the current git branch (`dev` is the active development branch).
3. Run `cargo build` to verify the workspace compiles cleanly.
4. Note the number of workspace tests: ~470 across all crates.
5. Review any open TODO items or known architectural violations from the last session.
