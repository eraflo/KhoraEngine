# Transform maths

<p class="kp-lede">A box in a game can be moved, turned and stretched, and all three are kept together in one table of numbers. Reading the turn back out of that table, or the way an arrow on the box points, is easy while the box keeps its shape. This page says what Khora does when it does not: a box squashed flat, mirrored, or so small or so large that the computer's rounding starts to matter.</p>

The maths behind `khora_core::math`'s transforms: how a rotation is read out of a
scaled matrix, how a direction is carried through one, when a matrix counts as
invertible, and the rounding floor all three rely on. For how transforms are used,
see [Spawn entities and move them](../how-to/spawn-and-transform.md); for the
batch kernels that compose them eight at a time, see
[AGDF](../concepts/agdf.md#layer-2--the-field-soa-substrate-and-why-it-is-the-lever).

## Notation

An affine transform (`AffineTransform`, the content of a `GlobalTransform`) is a
4 × 4 matrix *M* whose last row is *(0, 0, 0, 1)*. Its linear part *A* is the 3 × 3
block whose columns are the transformed axes: *r* (right, X), *u* (up, Y), *f*
(forward, Z). Its translation is *t*. For a vector *v*, *|v|* is its length and
*v̂ = v / |v|* its direction. Single-precision (`f32`) rounding has a unit of
about *2⁻²⁴ ≈ 6 · 10⁻⁸*.

A transform built from a translation, a rotation *R* and a scale *S = diag(sₓ, s_y,
s_z)* has *A = R S*: each column is a rotated axis times its scale.

## The rounding floor

A product of `f32` matrices leaves a **residue** of about 10⁻⁷ of the longest axis
in an axis meant to be exactly zero. A quarter turn under a parent flattened to zero
leaves one. Below that, an axis carries neither a direction nor a sign: it is noise.
So every test below compares lengths **to each other**, never to an absolute size:

- an axis is zero when *|a| ≤ ν · max(|r|, |u|, |f|)*, with *ν = 10⁻⁶*
  (`NOISE_FLOOR`);
- two unit directions are parallel when the length of their cross product is at
  most 10⁻⁶ (`PARALLEL_SINE`).

A vector is made unit by first dividing it by its largest component, then by its
length. Squaring a millimetre's component, or a kilometre's, then neither
underflows nor overflows, so a millimetre-scale model behaves as a metre-scale one.

## Reading the rotation

`AffineTransform::rotation` returns the turn of *A* as a unit quaternion, whatever
its scale.

1. **The unit basis.** Each column is made unit. One column at the rounding floor
   (an axis scaled to zero) is rebuilt as the right-handed cross product of the two
   others, in cyclic order: *r = u × f*, *u = f × r*, *f = r × u*.
2. **Forward is kept exactly.** A sheared basis keeps where it looks: *f̂* is kept.
3. **Up is made perpendicular to it**, by removing its component along *f̂*
   (one Gram–Schmidt step): *u⊥ = u − (u · f̂) f̂*.
4. **Right is derived**, *r̂ = û⊥ × f̂*, and up again, *û = f̂ × r̂*. If up is
   parallel to forward, the twist comes from right instead: right is made
   perpendicular to forward, and up and right are derived from it.
5. The orthonormal basis *(r̂, û, f̂)* becomes the quaternion.

It gives the identity when the basis is not finite, when two axes are zero, or when
the two that survive are parallel.

**Theorem (scale-free).** Let *A = R S* with *R* a rotation and *S* diagonal with
positive entries. Then `rotation(A) = R`, up to rounding.

*Proof.* The columns of *A* are *sᵢ Rᵢ*, so their unit directions are the columns
*Rᵢ* of *R*: orthonormal. Step 3 removes nothing (*u · f̂ = 0*), step 4 gives
*r̂ = R_y × R_z = R_x* because *R* is right-handed, and *û = R_z × R_x = R_y*. The
basis is *R*. ∎

**Mirrors.** If *det A < 0* (an odd number of negative scales), no rotation equals
the normalised basis. Since right is *derived* from up and forward, the result is
the rotation whose basis is *(−r̂, û, f̂)*: the mirror is put on the first axis.
Together with `scale()`, which reports magnitudes, this is the decomposition
*A = R · diag(−|sₓ|, |s_y|, |s_z|)*. When a mirrored transform is turned back into a
`Transform` (a body released by physics), the signs of the entity's authored scale
are kept when they describe the same mirror.

## Carrying a direction

`AffineTransform::transform_direction(ℓ)` returns the unit world direction a local
direction *ℓ* points along:

*d = A ℓ̂ / |A ℓ̂|*

It is the linear part, so under an uneven scale it leans with the scaled axes as
the drawn geometry does; it is not the rotation applied to *ℓ*. (A surface normal
transforms by *(A⁻¹)ᵀ* instead.) It is computed on *A* divided by its largest
entry, which changes no direction and keeps the sum from overflowing.

- *ℓ* zero or not finite: *d = 0*.
- *A* not finite or zero: *d = rotation(A) ℓ̂*.
- *|A ℓ̂|* at the rounding floor of the longest column — *ℓ* lies along an axis
  scaled to zero, so its image points nowhere: *d = rotation(A) ℓ̂*.

Under a pure rotation, *d = R ℓ̂*; under a uniform scale *s > 0*, *d = R ℓ̂* too,
since *s* cancels.

## When a matrix is invertible

A matrix whose determinant is small is not necessarily close to singular: a model
exported in centimetres has a scale of 0.01 and a determinant of 10⁻⁶, and inverts
exactly. What says how close the columns are to a lower-dimensional space is the
determinant **relative to the columns' lengths**.

**Hadamard's inequality.** For a square matrix with columns *c₁, …, cₙ*,
*|det| ≤ ∏ |cᵢ|*, with equality exactly when the columns are orthogonal.

*Sketch.* *|det|* is the volume of the parallelepiped the columns span. Removing
from each column its components along the previous ones (Gram–Schmidt) keeps the
volume and only shortens the columns; the volume of a box with orthogonal edges is
the product of their lengths. ∎

So the ratio *ρ = |det| / ∏ |cᵢ|* lies in *[0, 1]*, is 1 for orthogonal columns and
0 for dependent ones, and does not change when a column is scaled. `Mat3::inverse`,
`Mat4::inverse` and `Mat4::affine_inverse` (on its 3 × 3 part) accept a matrix
when:

- the determinant and the product of the lengths are finite;
- *|det| > ε ∏ |cᵢ|*, with *ε = 10⁻⁵* (`EPSILON`), that is *ρ > ε*;
- *1 / det* is finite, so the inverse holds no infinity.

The test is the same for a matrix and for any non-zero rescaling of its columns,
so it accepts a centimetre-scale transform and refuses a flattened one at any size.

## Next steps

- [Spawn entities and move them](../how-to/spawn-and-transform.md) — `rotation()` and
  `transform_direction()` in game code.
- [Physics](../concepts/physics.md) — the release that turns a drawn pose back into a
  `Transform`.
- [AGDF](../concepts/agdf.md) — the SIMD kernels and what was measured of them.
