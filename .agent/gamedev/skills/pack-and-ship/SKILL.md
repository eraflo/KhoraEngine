---
name: pack-and-ship
description: Builds a shippable Khora game with the editor's Build Game — a signed asset pack beside a stamped binary — and verifies it starts. Use when preparing a release build or diagnosing a built game that refuses to start.
---

# Build and ship

A shipped game is a binary stamped with the developer's public key, beside a pack signed with the
private key. Every start checks one against the other.

## Steps
1. **Trust** — Build Game is refused in a restricted project (building compiles and runs its code). The
   user trusts the project in the hub or the editor; never decide it for them.
2. **Build** — in the editor, *Build → Build Game…*, pick the target. The strategy follows the project:

   | Project | Strategy | Binary |
   |---|---|---|
   | no `Cargo.toml` (data-only) | runtime stamp | the prebuilt `khora-runtime` for the target, renamed after the project — any target |
   | `Cargo.toml` (native code) | cargo build | `cargo build --release` of the project — host target only |

   Both pack `assets/` (scripts included) and write `<project>/dist/<target>/`: the binary, `data.pack`,
   `index.bin`, `manifest.bin` (signed) and `runtime.json` (the scene to load first).
3. **Run** the binary from `dist/<target>/`. It verifies `manifest.bin` with its stamped key, the index
   against the manifest, and each asset against its hash as it loads.

## What refuses a start

| Beside the binary | Binary with no key (dev run) | Stamped binary (built game) |
|---|---|---|
| no pack | starts, reads loose `assets/` | refused |
| a pack | refused (`PackSealError::Unstamped`) | starts only if the seal verifies |

An asset whose bytes changed after the build is refused and fails its load; a pack whose index or
manifest changed refuses the start. A stamped binary never reads loose files, and a script left beside
the pack is not the game's.

## Rules — shipping is outward-facing
- **Ask before building or distributing** (permission model in [`../../RULES.md`](../../RULES.md)).
- **The signing key stays home**: `~/.khora/signing/pack.key`, made once and never replaced; never in a
  project, never committed.
- **No secrets in the build.** A pack and a binary are readable by players. Run the
  [`security-auditor`](../../agents/security-auditor.md) first.
- The seal proves the assets are the ones the binary was built with, not who built the binary.
- Report honestly what was built and whether it was run.
