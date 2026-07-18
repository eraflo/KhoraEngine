# Math — reference

Domain knowledge for Khora math. Consulted during Research (dispatch the `codebase-*` subagents to
apply it to concrete files). Follow [`../conventions.md`](../conventions.md) §7.

## Scope
Linear algebra (`Vec2/3/4`, `Mat3/4`, `Quaternion`, `Aabb`, `LinearRgba`), numerical stability, and the
explicit-SIMD hot-loop kernels.

## Key files
- Math module: `crates/khora-core/src/math/`.
- SIMD: `crates/khora-core/src/math/simd.rs` — `TrsBatchSoa` (field-split AoSoA), `wide::f32x8` (8-wide AVX)
  for quaternion→matrix expansion, with scalar twins for the ragged tail and validation.

## Hard rules
- **All** engine math goes through `khora_core::math` — never raw `glam`. Extend the module when needed.
- Right-handed, column-major, **Y-up**.
- SIMD kernels own their physical tiling; callers fill from any source. Keep a scalar twin for correctness
  and the tail; assert equivalence in tests.
- Document non-trivial derivations with the source paper/formula.

Use codegraph to find every caller before changing a public math signature (it ripples widely).

## Skills
- [`build-and-test`](../skills/build-and-test/SKILL.md) — math has dense unit tests; run them.
- [`debug-frame`](../skills/debug-frame/SKILL.md) — when a numerical issue surfaces at runtime (transform/shadow precision).
