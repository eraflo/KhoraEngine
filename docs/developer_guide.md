# KhoraEngine Developer Guide

This guide provides comprehensive information for developers working with or contributing to KhoraEngine.

## Table of Contents

1. [Getting Started](#getting-started)
2. [Project Structure](#project-structure) 
3. [Core Concepts](#core-concepts)
4. [Development Workflow](#development-workflow)
5. [Extending the Engine](#extending-the-engine)
6. [Testing](#testing)
7. [Performance Considerations](#performance-considerations)
8. [Debugging Tips](#debugging-tips)
9. [Next Steps](#next-steps)

## Getting Started

### Prerequisites

- Rust 1.70+ (recommended latest stable)
- A graphics driver supporting Vulkan, DirectX 12, Metal, or OpenGL
- Git for version control

### Initial Setup

```bash
# Clone the repository
git clone https://github.com/eraflo/KhoraEngine.git
cd KhoraEngine

# Verify everything builds
cargo check --workspace

# Run tests
cargo test --workspace

# Build and run the sandbox application
cargo run --bin sandbox
```

### Development Environment

#### Recommended VS Code Extensions

- `rust-analyzer` - Rust language support
- `CodeLLDB` - Debugging support
- `Better TOML` - TOML file syntax highlighting
- `GitLens` - Enhanced Git integration

#### Pre-commit Verification

Before pushing changes, run the verification script:

```bash
# Windows (PowerShell)
powershell -ExecutionPolicy Bypass -File .\verify.ps1 --install-tools --full

# Linux/macOS
./verify.sh --fix --install-tools --full
```

## Project Structure

```
KhoraEngine/
├── khora_engine_core/          # Core engine library
│   ├── src/
│   │   ├── core/              # Engine runtime (loop, monitoring, timers)
│   │   ├── event/             # Event system and bus
│   │   ├── math/              # Mathematical primitives
│   │   ├── memory/            # Memory management and tracking
│   │   ├── subsystems/        # Pluggable subsystems
│   │   │   ├── input.rs       # Input handling
│   │   │   └── renderer/      # Rendering subsystem
│   │   └── window/            # Window management
│   └── Cargo.toml
├── sandbox/                   # Example application
├── docs/                      # Documentation
├── assets/                    # Asset files (logos, etc.)
└── target/                    # Build artifacts
```

### Key Modules

#### `core` Module
- **`engine.rs`**: Main engine orchestrator with the application loop
- **`monitoring.rs`**: Performance monitoring and resource tracking  
- **`timer.rs`**: CPU timing utilities (`Stopwatch`)
- **`utils/`**: Generic utilities (bitflags macro, etc.)

#### `math` Module
- **Vector types**: `Vec2`, `Vec3`, `Vec4` with SIMD optimizations
- **Matrix types**: `Mat3`, `Mat4` for transformations
- **`Quaternion`**: Rotation representation
- **`LinearRgba`**: Color representation
- **Geometric primitives**: `Aabb`, extents, origins

#### `subsystems` Module
- **`input.rs`**: Cross-platform input event handling
- **`renderer/`**: Complete rendering abstraction layer

#### `event` Module
- **Event bus**: Decoupled publish-subscribe system
- **Engine events**: Core engine lifecycle events

#### `memory` Module
- **`SaaTrackingAllocator`**: Memory allocation tracking for SAA insights

#### `window` Module
- **Window management**: Cross-platform window creation and event handling

## Core Concepts

### Symbiotic Adaptive Architecture (SAA)

KhoraEngine is designed around the SAA philosophy:

- **Intelligent Subsystem Agents (ISAs)**: Subsystems that can adapt their behavior
- **Dynamic Context Core (DCC)**: Central coordinator for adaptive decisions
- **Goal-Oriented Resource Negotiation (GORNA)**: Resource allocation negotiation
- **Context-aware adaptation**: Real-time performance and resource-driven optimization

### Event-Driven Architecture

The engine uses an event bus for loose coupling:

```rust
use khora_engine_core::{Engine, EngineEvent, KhoraInputEvent};

// Subscribe to events
let mut engine = Engine::new(window)?;

// Handle events in your main loop
for event in engine.poll_events() {
    match event {
        EngineEvent::Input(input_event) => {
            // Handle input
        }
        EngineEvent::Render(render_stats) => {
            // Handle render completion
        }
        // ... other events
    }
}
```

### Resource Management

All resources are tracked for SAA decision-making:
- **Memory allocation**: Tracked via `SaaTrackingAllocator`
- **VRAM usage**: Monitored by graphics device
- **CPU timing**: Measured with built-in profiling
- **GPU timing**: Captured via timestamp queries

## Development Workflow

### Branch Strategy

- `main`: Stable releases and major milestones
- `develop`: Integration branch for new features
- Feature branches: `XX-feature-description` (where XX is issue number)

### Issue-Driven Development

1. Create/assign yourself to a GitHub issue
2. Create a feature branch: `git checkout -b 42-feature-new-cool-thing`
3. Implement with tests
4. Run verification: `./verify.sh --full`
5. Create pull request with filled-out template
6. Address review feedback
7. Merge after approval

### Code Style

- Follow `rustfmt` formatting (enforced by CI)
- Use `clippy` suggestions (warnings as errors)
- Prefer descriptive names over comments
- Document public APIs with `///` doc comments
- Use `Result` types over panics for error handling

### Testing Strategy

```bash
# Run all tests
cargo test --workspace

# Run specific test
cargo test --package khora_engine_core math::tests

# Run with output
cargo test -- --nocapture

# Run tests with extra threads
cargo test -- --test-threads=1
```

## Extending the Engine

### Adding a New Subsystem

1. **Create the subsystem module**:
   ```rust
   // khora_engine_core/src/subsystems/audio.rs
   use crate::event::EngineEvent;
   
   pub struct AudioSystem {
       // ... state
   }
   
   impl AudioSystem {
       pub fn new() -> Self {
           // ... initialization
       }
       
       pub fn update(&mut self) -> Vec<EngineEvent> {
           // ... update logic, return events
       }
   }
   ```

2. **Update the subsystems module**:
   ```rust
   // khora_engine_core/src/subsystems/mod.rs
   pub mod audio;
   pub use audio::AudioSystem;
   ```

3. **Integrate with the engine**:
   ```rust
   // khora_engine_core/src/core/engine.rs
   // Add to Engine struct and update methods
   ```

4. **Add tests and documentation**

### Creating Custom Events

```rust
use crate::event::EngineEvent;

#[derive(Debug, Clone)]
pub enum CustomEvent {
    AudioPlayed { sound_id: u32 },
    NetworkConnected { peer_id: String },
}

// Convert to EngineEvent
let event = EngineEvent::Custom(Box::new(CustomEvent::AudioPlayed { sound_id: 123 }));
```

### Implementing Performance-Aware Components

Design components with performance monitoring in mind:

1. **Resource monitoring integration**:
   ```rust
   use khora_engine_core::core::monitoring::ResourceMonitor;
   
   impl ResourceMonitor for MyComponent {
       fn get_resource_usage(&self) -> ResourceUsage {
           ResourceUsage {
               cpu_time_ms: self.last_frame_time,
               memory_bytes: self.calculate_memory_usage(),
               custom_metrics: HashMap::new(),
           }
       }
   }
   ```

2. **Multiple execution strategies**:
   ```rust
   pub enum RenderStrategy {
       HighQuality,
       Balanced,
       Performance,
   }
   ```

3. **Performance metrics collection**:
   ```rust
   pub struct SubsystemMetrics {
       pub cpu_time_ms: f32,
       pub memory_used_mb: f32,
       pub operations_per_second: f32,
   }
   ```

## Testing

KhoraEngine follows Rust's standard testing conventions with tests placed directly in source files.

### Unit Tests

Place tests at the end of each source file using the `#[cfg(test)]` attribute:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vector_addition() {
        let v1 = Vec3::new(1.0, 2.0, 3.0);
        let v2 = Vec3::new(4.0, 5.0, 6.0);
        let result = v1 + v2;
        assert_eq!(result, Vec3::new(5.0, 7.0, 9.0));
    }

    #[test]
    fn test_vector_normalization() {
        let v = Vec3::new(1.0, 0.0, 0.0);
        let normalized = v.normalize();
        assert_eq!(normalized, Vec3::new(1.0, 0.0, 0.0));
    }
}
```

### Running Tests

```bash
# Run all tests
cargo test

# Run tests for a specific module
cargo test math::vector

# Run tests with output
cargo test -- --nocapture

# Run tests in parallel
cargo test -- --test-threads=4
```

### Test Organization

**Current project structure:**
- Tests are embedded in source files at the end
- Use `#[cfg(test)]` to exclude from release builds
- Import parent module with `use super::*;`
- Group related tests in the same `tests` module

**Example from the math module:**
```rust
// At the end of khora_engine_core/src/math/vector.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::approx_eq;

    fn vec3_approx_eq(a: Vec3, b: Vec3) -> bool {
        approx_eq(a.x, b.x) && approx_eq(a.y, b.y) && approx_eq(a.z, b.z)
    }

    #[test]
    fn test_vec3_cross_product() {
        let v1 = Vec3::new(1.0, 0.0, 0.0);
        let v2 = Vec3::new(0.0, 1.0, 0.0);
        let cross = v1.cross(v2);
        assert!(vec3_approx_eq(cross, Vec3::new(0.0, 0.0, 1.0)));
    }
}
```

### Performance Tests

Use `criterion` for benchmarks:

```bash
cargo bench
```

## Performance Considerations

### Memory Management

- Use the provided `SaaTrackingAllocator` to monitor allocations
- Prefer stack allocation for small, short-lived data
- Use `Arc` and `Rc` judiciously - prefer ownership transfer
- Monitor memory usage via engine statistics

### CPU Performance

- Profile with `cargo flamegraph` or similar tools
- Use `Stopwatch` for timing critical sections
- Prefer data-oriented design (structs of arrays vs arrays of structs)
- Use SIMD when beneficial (math types already optimized)

### GPU Performance

KhoraEngine provides comprehensive GPU performance monitoring through multiple layers:

#### GPU Timestamp Profiling
- Enable GPU timestamp profiling with `RenderSettings.enable_gpu_timestamps = true`
- Monitor GPU frame time via `RenderStats.gpu_main_pass_time_ms` and `gpu_frame_total_time_ms`
- Access raw timestamp data through the timestamp profiler

#### ResourceMonitor for GPU Performance
- Use `WgpuGpuPerformanceMonitor` for detailed GPU timing analysis
- Access microsecond-precision metrics for main pass and frame totals
- Integrate with external monitoring systems through the `ResourceMonitor` trait

```rust
// Access GPU performance monitor
if let Some(gpu_monitor) = render_system.gpu_performance_monitor() {
    if let Some(report) = gpu_monitor.get_gpu_performance_report() {
        println!("Main pass: {}μs, Frame total: {}μs", 
                 report.main_pass_duration_us.unwrap_or(0),
                 report.frame_total_duration_us.unwrap_or(0));
    }
}
```

#### Performance Optimization Tips
- Batch draw calls when possible
- Minimize state changes
- Use appropriate texture formats for your content
- Profile regularly with `cargo run --release` for accurate GPU timings
- Consider MSAA impact on fill rate

For detailed GPU monitoring documentation, see [GPU Performance Monitoring](gpu_performance_monitoring.md).

### Threading

- Current engine is single-threaded by design
- Ensure thread-safety with `Send + Sync` traits for components that need it

## Debugging Tips

### Logging

Use structured logging throughout:

```rust
use log::{info, warn, error, debug, trace};

// Different log levels
trace!("Detailed execution flow");
debug!("Debug information: {:?}", some_value);
info!("General information");
warn!("Warning: {}", warning_message);
error!("Error occurred: {}", error);
```

Set log levels:
```bash
RUST_LOG=khora_engine_core=debug cargo run --bin sandbox
RUST_LOG=trace cargo run --bin sandbox  # Very verbose
```

### Graphics Debugging

Enable graphics debugging:
- Use RenderDoc for frame capture
- Enable validation layers in debug builds
- Monitor GPU timing with built-in profiling

### Memory Debugging

- Use `valgrind` on Linux for memory leak detection
- Monitor allocation patterns via `SaaTrackingAllocator`
- Use `heaptrack` for allocation profiling

### Common Issues

1. **Window creation fails**: Check graphics drivers and available backends
2. **Shader compilation errors**: Verify WGSL syntax and entry points
3. **Performance issues**: Profile with built-in timing, check allocations
4. **Build failures**: Run `cargo clean` and retry, check Rust version

## Next Steps

1. **Read the architecture documentation**: `docs/architecture_design.md`
2. **Explore the rendering system**: `docs/rendering/`
3. **Run the verification script**: Ensure your changes pass all checks
4. **Join discussions**: Participate in GitHub Discussions for design decisions
5. **Contribute**: Look for "good first issue" labels on GitHub

For more specific information, refer to the individual module documentation and the architectural design documents.
