---
name: pack-and-ship
description: Builds, packs assets, and produces a shippable runtime for a Khora game. Use when preparing a release build, bundling assets into a pack, or producing the player binary.
---

# Pack and ship

A shipped game = a release binary that boots a **packed** asset bundle via `run_default` / `khora-runtime`.

## Steps
1. **Build release**: `cargo build --release`.
2. **Pack assets**: bundle your `assets/` into a pack with `PackBuilder` (LZ4-compressed). The engine repo's
   `cargo xtask assets pack` and the **hub** (`cargo run -p hub`) also drive packing.
3. **Runtime**: the packed game boots through `run_default(...)` (the `khora-runtime` crate is the generic
   player stamped with your pack). Optionally enable integrity verification (`verify_integrity`) so the
   runtime checks a `manifest.bin` next to the executable.
4. **Verify the build**: run the release binary, confirm it loads the pack and renders.

## Rules — shipping is outward-facing
- **Ask before producing or distributing a build** (permission model in [`../../RULES.md`](../../RULES.md)).
- **No secrets in the build.** A pack/binary is readable by players — keep keys server-side, hand the client
  short-lived tokens at runtime. Run the [`security-auditor`](../../agents/security-auditor.md) first.
- Report honestly what was built and whether it was verified.
