# Math Module Documentation

The `math` module provides fundamental mathematical types and operations optimized for game engine use. All types are designed for performance with SIMD optimizations where applicable.

## Table of Contents

1. [Overview](#overview)
2. [Vector Types](#vector-types)
3. [Matrix Types](#matrix-types)
4. [Quaternions](#quaternions)
5. [Colors](#colors)
6. [Geometric Types](#geometric-types)
7. [Utility Functions](#utility-functions)
8. [Performance Notes](#performance-notes)
9. [Usage Examples](#usage-examples)

## Overview

The math module provides:
- **Vectors**: `Vec2`, `Vec3`, `Vec4` for 2D/3D mathematics
- **Matrices**: `Mat3`, `Mat4` for transformations
- **Quaternions**: `Quaternion` for rotations
- **Colors**: `LinearRgba` for color representation
- **Geometric primitives**: `Aabb`, extents, origins
- **Constants**: Mathematical constants and conversion factors

All types implement standard traits like `Debug`, `Clone`, `Copy`, `PartialEq`, and provide mathematical operations through operator overloading.

## Vector Types

### Vec2

2D vector for positions, directions, and texture coordinates.

```rust
use khora_engine_core::math::Vec2;

// Construction
let v = Vec2::new(1.0, 2.0);
let zero = Vec2::ZERO;
let unit_x = Vec2::X;
let unit_y = Vec2::Y;

// Operations
let a = Vec2::new(1.0, 2.0);
let b = Vec2::new(3.0, 4.0);
let sum = a + b;           // Vector addition
let scaled = a * 2.0;      // Scalar multiplication
let dot = a.dot(b);        // Dot product
let length = a.length();   // Vector magnitude
let normalized = a.normalize(); // Unit vector
```

**Common Methods:**
- `length()` - Vector magnitude
- `length_squared()` - Squared magnitude (faster, avoids sqrt)
- `normalize()` - Returns unit vector
- `distance(other)` - Distance to another point
- `dot(other)` - Dot product
- `lerp(other, t)` - Linear interpolation

### Vec3

3D vector for positions, directions, normals, and colors.

```rust
use khora_engine_core::math::Vec3;

// Construction
let v = Vec3::new(1.0, 2.0, 3.0);
let zero = Vec3::ZERO;
let unit_x = Vec3::X;
let unit_y = Vec3::Y;
let unit_z = Vec3::Z;

// 3D-specific operations
let a = Vec3::new(1.0, 0.0, 0.0);
let b = Vec3::new(0.0, 1.0, 0.0);
let cross = a.cross(b);    // Cross product (results in (0, 0, 1))

// Convert from Vec2
let v2 = Vec2::new(1.0, 2.0);
let v3 = Vec3::from(v2);   // Z component = 0.0
```

**Unique Methods:**
- `cross(other)` - Cross product (3D only)
- `reflect(normal)` - Reflect vector across normal

### Vec4

4D vector for homogeneous coordinates, RGBA colors, and quaternions.

```rust
use khora_engine_core::math::Vec4;

// Construction
let v = Vec4::new(1.0, 2.0, 3.0, 1.0);
let zero = Vec4::ZERO;

// Often used for homogeneous coordinates
let position = Vec4::new(x, y, z, 1.0);  // Point
let direction = Vec4::new(x, y, z, 0.0); // Vector

// Convert from smaller vectors
let v3 = Vec3::new(1.0, 2.0, 3.0);
let v4 = Vec4::from(v3);   // W component = 0.0
```

## Matrix Types

### Mat3

3×3 matrix for 2D transformations and 3D rotations.

```rust
use khora_engine_core::math::{Mat3, Vec3};

// Construction
let identity = Mat3::IDENTITY;
let zero = Mat3::ZERO;

// 2D transformations
let translation = Mat3::from_translation(Vec2::new(10.0, 20.0));
let rotation = Mat3::from_rotation(PI / 4.0); // 45 degrees
let scale = Mat3::from_scale(Vec2::new(2.0, 3.0));

// Combination
let transform = translation * rotation * scale;

// Apply to point
let point = Vec2::new(1.0, 1.0);
let transformed = transform.transform_point(point);
```

**Key Methods:**
- `from_translation(Vec2)` - Translation matrix
- `from_rotation(angle)` - Rotation matrix
- `from_scale(Vec2)` - Scale matrix
- `transform_point(Vec2)` - Transform a point
- `transform_vector(Vec2)` - Transform a vector (no translation)
- `determinant()` - Matrix determinant
- `inverse()` - Matrix inverse (returns `Option<Mat3>`)

### Mat4

4×4 matrix for 3D transformations, view, and projection matrices.

```rust
use khora_engine_core::math::{Mat4, Vec3, Quaternion};

// Construction
let identity = Mat4::IDENTITY;

// 3D transformations
let translation = Mat4::from_translation(Vec3::new(10.0, 20.0, 30.0));
let rotation = Mat4::from_quaternion(quaternion);
let scale = Mat4::from_scale(Vec3::new(2.0, 3.0, 4.0));

// View matrix
let view = Mat4::look_at(
    Vec3::new(0.0, 0.0, 5.0), // eye position
    Vec3::new(0.0, 0.0, 0.0), // target
    Vec3::new(0.0, 1.0, 0.0)  // up vector
);

// Projection matrices
let perspective = Mat4::perspective(
    75.0_f32.to_radians(), // field of view
    16.0 / 9.0,           // aspect ratio
    0.1,                  // near plane
    100.0                 // far plane
);

let orthographic = Mat4::orthographic(
    -10.0, 10.0,  // left, right
    -10.0, 10.0,  // bottom, top
    0.1, 100.0    // near, far
);
```

**Key Methods:**
- `from_translation(Vec3)` - Translation matrix
- `from_quaternion(Quaternion)` - Rotation from quaternion
- `from_scale(Vec3)` - Scale matrix
- `from_rotation_x/y/z(angle)` - Rotation around specific axis
- `look_at(eye, target, up)` - View matrix
- `perspective(fov, aspect, near, far)` - Perspective projection
- `orthographic(...)` - Orthographic projection
- `transform_point(Vec3)` - Transform 3D point
- `transform_vector(Vec3)` - Transform 3D vector

## Quaternions

Quaternions provide stable rotation representation without gimbal lock.

```rust
use khora_engine_core::math::{Quaternion, Vec3};

// Construction
let identity = Quaternion::IDENTITY;
let rotation = Quaternion::from_axis_angle(Vec3::Y, PI / 2.0); // 90° around Y

// From Euler angles (careful of order!)
let euler = Quaternion::from_euler(roll, pitch, yaw);

// Interpolation
let q1 = Quaternion::from_axis_angle(Vec3::Y, 0.0);
let q2 = Quaternion::from_axis_angle(Vec3::Y, PI);
let interpolated = q1.slerp(q2, 0.5); // Spherical linear interpolation

// Apply rotation
let vector = Vec3::new(1.0, 0.0, 0.0);
let rotated = rotation * vector;

// Combine rotations
let combined = rotation1 * rotation2;
```

**Key Methods:**
- `from_axis_angle(axis, angle)` - Create from axis and angle
- `from_euler(x, y, z)` - Create from Euler angles
- `slerp(other, t)` - Spherical linear interpolation
- `inverse()` - Inverse rotation
- `dot(other)` - Quaternion dot product
- `length()` - Quaternion magnitude
- `normalize()` - Normalize quaternion

## Colors

### LinearRgba

Linear color space RGBA representation for graphics operations.

```rust
use khora_engine_core::math::LinearRgba;

// Construction
let red = LinearRgba::RED;
let green = LinearRgba::GREEN;
let blue = LinearRgba::BLUE;
let white = LinearRgba::WHITE;
let black = LinearRgba::BLACK;
let transparent = LinearRgba::TRANSPARENT;

// Custom colors
let orange = LinearRgba::new(1.0, 0.5, 0.0, 1.0);

// Operations
let mixed = red.mix(blue, 0.5);        // Mix colors
let brighter = orange * 1.5;          // Scale brightness
let with_alpha = orange.with_alpha(0.5); // Change alpha
```

**Key Methods:**
- `new(r, g, b, a)` - Create color
- `mix(other, factor)` - Mix two colors
- `with_alpha(alpha)` - Set alpha channel
- `to_srgb()` - Convert to sRGB (if implemented)

## Geometric Types

### Extents

Dimensional specifications for textures, buffers, and regions.

```rust
use khora_engine_core::math::{Extent1D, Extent2D, Extent3D};

// 1D extent (buffer size, texture width)
let buffer_size = Extent1D { width: 1024 };

// 2D extent (texture size, screen resolution)
let texture_size = Extent2D { 
    width: 1920, 
    height: 1080 
};

// 3D extent (volume texture, array layers)
let volume = Extent3D { 
    width: 256, 
    height: 256, 
    depth_or_array_layers: 16 
};
```

### Origins

Offset specifications for texture operations and regions.

```rust
use khora_engine_core::math::{Origin2D, Origin3D};

// 2D origin (texture coordinates, screen position)
let offset_2d = Origin2D { x: 100, y: 200 };

// 3D origin (volume texture offset)
let offset_3d = Origin3D { x: 10, y: 20, z: 5 };
```

### Aabb (Axis-Aligned Bounding Box)

Bounding volume for collision detection and culling.

```rust
use khora_engine_core::math::{Aabb, Vec3};

// Construction
let min = Vec3::new(-1.0, -1.0, -1.0);
let max = Vec3::new(1.0, 1.0, 1.0);
let aabb = Aabb::new(min, max);

// Queries
let center = aabb.center();
let size = aabb.size();
let contains_point = aabb.contains(Vec3::new(0.5, 0.0, 0.0));

// Operations
let expanded = aabb.expand(Vec3::new(2.0, 0.0, 3.0));
let union = aabb.union(&other_aabb);
```

## Utility Functions

```rust
use khora_engine_core::math::{degrees_to_radians, radians_to_degrees, clamp, lerp};

// Angle conversion
let radians = degrees_to_radians(90.0);  // π/2
let degrees = radians_to_degrees(PI);    // 180.0

// Mathematical utilities
let clamped = clamp(value, 0.0, 1.0);          // Clamp to range
let interpolated = lerp(start, end, 0.5);       // Linear interpolation
let smooth = smoothstep(0.0, 1.0, 0.7);       // Smooth interpolation
```

**Available Functions:**
- `degrees_to_radians(degrees)` - Convert degrees to radians
- `radians_to_degrees(radians)` - Convert radians to degrees
- `clamp(value, min, max)` - Clamp value to range
- `lerp(a, b, t)` - Linear interpolation
- `smoothstep(edge0, edge1, x)` - Smooth interpolation
- `step(edge, x)` - Step function

## Performance Notes

### SIMD Optimizations

- Vector operations use SIMD when available
- Matrix multiplication is optimized for common cases
- Prefer batch operations over individual element access

### Memory Layout

- All types are `#[repr(C)]` for predictable layout
- Vectors and matrices are tightly packed
- No padding between components

### Best Practices

```rust
// ✅ Good: Batch operations
let transformed: Vec<Vec3> = positions
    .iter()
    .map(|&pos| transform.transform_point(pos))
    .collect();

// ✅ Good: Avoid unnecessary sqrt
if vector.length_squared() > threshold_squared {
    // ...
}

// ✅ Good: Reuse normalized vectors
let normalized = vector.normalize();
let dot1 = normalized.dot(other1);
let dot2 = normalized.dot(other2);

// ❌ Avoid: Repeated normalization
let dot1 = vector.normalize().dot(other1);
let dot2 = vector.normalize().dot(other2);
```

### Common Optimizations

1. **Use `length_squared()` instead of `length()`** when comparing distances
2. **Batch matrix operations** rather than individual transforms
3. **Cache normalized vectors** when used multiple times
4. **Prefer quaternions over Euler angles** for rotations
5. **Use appropriate precision** (f32 vs f64 based on needs)

## Usage Examples

### 3D Transformation Pipeline

```rust
use khora_engine_core::math::{Mat4, Vec3, Quaternion};

// Object transformation
let position = Vec3::new(10.0, 5.0, 0.0);
let rotation = Quaternion::from_axis_angle(Vec3::Y, PI / 4.0);
let scale = Vec3::new(2.0, 1.0, 2.0);

let model_matrix = Mat4::from_translation(position) 
    * Mat4::from_quaternion(rotation) 
    * Mat4::from_scale(scale);

// Camera setup
let camera_pos = Vec3::new(0.0, 10.0, 20.0);
let target = Vec3::new(0.0, 0.0, 0.0);
let up = Vec3::Y;

let view_matrix = Mat4::look_at(camera_pos, target, up);

// Projection setup
let fov = 75.0_f32.to_radians();
let aspect_ratio = 1920.0 / 1080.0;
let near = 0.1;
let far = 100.0;

let projection_matrix = Mat4::perspective(fov, aspect_ratio, near, far);

// Combined transformation
let mvp_matrix = projection_matrix * view_matrix * model_matrix;
```

### Physics/Game Logic

```rust
use khora_engine_core::math::{Vec3, Aabb};

// Velocity integration
let mut position = Vec3::new(0.0, 0.0, 0.0);
let velocity = Vec3::new(1.0, 0.0, 0.0);
let acceleration = Vec3::new(0.0, -9.81, 0.0); // gravity
let dt = 0.016; // 60 FPS

// Update physics
velocity += acceleration * dt;
position += velocity * dt;

// Collision detection
let player_bounds = Aabb::new(
    position - Vec3::new(0.5, 1.0, 0.5),
    position + Vec3::new(0.5, 1.0, 0.5)
);

let wall_bounds = Aabb::new(
    Vec3::new(10.0, 0.0, -1.0),
    Vec3::new(11.0, 5.0, 1.0)
);

if player_bounds.intersects(&wall_bounds) {
    // Handle collision
}
```

### Color Manipulation

```rust
use khora_engine_core::math::LinearRgba;

// Create palette
let primary = LinearRgba::new(0.2, 0.6, 1.0, 1.0);   // Blue
let secondary = LinearRgba::new(1.0, 0.4, 0.2, 1.0); // Orange

// Generate gradient
let steps = 10;
let gradient: Vec<LinearRgba> = (0..steps)
    .map(|i| {
        let t = i as f32 / (steps - 1) as f32;
        primary.mix(secondary, t)
    })
    .collect();

// Lighting calculation
let base_color = LinearRgba::new(0.8, 0.6, 0.4, 1.0);
let light_intensity = 0.7;
let lit_color = base_color * light_intensity;
```

This documentation provides a comprehensive guide to the math module. For implementation details, refer to the source code in `khora_engine_core/src/math/`.
