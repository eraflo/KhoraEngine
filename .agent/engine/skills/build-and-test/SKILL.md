---
name: build-and-test
description: Validates an engine change by compiling and running the workspace test suite. Use after any code edit, before declaring work complete, or when the user asks to build, test, lint, or check the engine.
---

# Build and test

The validation loop for any change. **Primary command (95% of cases):**

```bash
cargo test --workspace
```

This builds everything and runs the full suite (~1650 tests today; treat the live count as truth). Deviate only when:

| Need | Command |
|---|---|
| Fast compile check only | `cargo build` |
| Lints | `cargo clippy --workspace` |
| A single crate's tests | `cargo test -p <crate>` (e.g. `khora-control`) |
| One test by name | `cargo test --workspace <test_name>` |
| Doc examples | `cargo test --doc` |
| Full CI gate (fmt+clippy+test+doc) | `cargo xtask all` |

## Definition of done
- Compiles with **zero warnings**; clippy clean.
- All workspace tests pass (0 failures). Report the live count, not a remembered one.
- For GPU work, also run `cargo run -p sandbox` once and confirm a clean frame loop (no Vulkan validation
  errors, scene renders).

## When tests fail
Report the failure honestly with the output. Investigate the root cause before patching; never paper over
a failing test by ignoring it. If a step was skipped (e.g. you didn't run the sandbox), say so.
