# Developer Guide

This guide provides comprehensive information for developers working on the Khora Engine project.

## Project Structure

### Core Engine (`khora_engine_core/`)

The core engine contains the fundamental systems and utilities:

- `core/` - Engine core systems (metrics, monitoring, utils)
- `event/` - Event system for inter-system communication
- `math/` - Mathematical utilities (vectors, matrices, quaternions)
- `memory/` - Memory management and allocation
- `subsystems/` - Rendering, input, and other subsystems
- `window/` - Window management

### Sandbox (`sandbox/`)

The sandbox project serves as a testing and development environment for the engine.

## Development Workflow

### Building the Project

```powershell
# Build all targets
cargo build

# Build in release mode
cargo build --release

# Build and run sandbox
cargo run --bin sandbox
```

### Running Tests

```powershell
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run specific test
cargo test test_name
```

### Code Quality

The project uses several tools to maintain code quality:

- **Clippy** for linting
- **Rustfmt** for formatting
- **Deny** for dependency auditing

```powershell
# Run clippy
cargo clippy

# Format code
cargo fmt

# Check dependencies
cargo deny check
```

## Core Systems

### Metrics System

The engine includes a comprehensive metrics system for performance monitoring:

```rust
use khora_engine_core::core::metrics::EngineMetrics;

// Create metrics system
let mut metrics = EngineMetrics::new();

// Register custom metrics
metrics.register_custom_counter("my_counter", "Description");
metrics.register_custom_gauge("my_gauge", "Description");

// Update metrics
metrics.increment_counter("my_counter", 1.0);
metrics.set_gauge("my_gauge", 42.0);
```

### Resource Monitoring

The resource monitoring system tracks various system resources:

#### Memory Monitoring

```rust
use khora_engine_core::core::resource_monitors::MemoryMonitor;

let monitor = MemoryMonitor::new("MainMemory".to_string());
monitor.update_from_bytes(1024 * 1024 * 100); // 100 MB

if let Some(report) = monitor.get_memory_report() {
    println!("Current memory usage: {} bytes", report.current_bytes);
}
```

#### GPU Monitoring

```rust
use khora_engine_core::core::resource_monitors::GpuMonitor;

let gpu_monitor = GpuMonitor::new("MainGPU".to_string());

// Update with render statistics
let stats = RenderStats {
    frame_number: 1,
    cpu_preparation_time_ms: 2.0,
    cpu_render_submission_time_ms: 0.5,
    gpu_main_pass_time_ms: 10.0,
    gpu_frame_total_time_ms: 15.0,
    draw_calls: 100,
    triangles_rendered: 50000,
    vram_usage_estimate_mb: 256.0,
};

gpu_monitor.update_from_frame_stats(&stats);

if let Some(report) = gpu_monitor.get_gpu_report() {
    println!("Frame {}: GPU time {}μs", 
             report.frame_number,
             report.frame_total_duration_us().unwrap_or(0));
}
```

### Event System

The event system enables communication between different engine components:

```rust
use khora_engine_core::event::EventBus;
use khora_engine_core::core::InternalEvent;

let event_bus = EventBus::new();
let sender = event_bus.sender();
let receiver = event_bus.receiver();

// Send events
sender.send(InternalEvent::Shutdown).unwrap();

// Receive events
if let Ok(event) = receiver.try_recv() {
    match event {
        InternalEvent::Shutdown => println!("Shutdown requested"),
        _ => {}
    }
}
```

### Math Utilities

The engine provides comprehensive mathematical utilities:

```rust
use khora_engine_core::math::{Vec3, Mat4, Quat};

// Vector operations
let v1 = Vec3::new(1.0, 2.0, 3.0);
let v2 = Vec3::new(4.0, 5.0, 6.0);
let dot_product = v1.dot(v2);
let cross_product = v1.cross(v2);

// Matrix operations
let transform = Mat4::from_translation(Vec3::new(10.0, 0.0, 0.0));
let rotation = Mat4::from_quat(Quat::from_axis_angle(Vec3::Y, 45.0_f32.to_radians()));
let combined = transform * rotation;

// Apply transformation
let point = Vec3::new(1.0, 0.0, 0.0);
let transformed = combined.transform_point3(point);
```

## Rendering System

### WGPU Backend

The engine uses WGPU as its primary rendering backend:

```rust
use khora_engine_core::subsystems::renderer::wgpu_impl::WgpuDevice;

// The rendering system is initialized automatically
// Custom shaders and pipelines can be created through the device interface
```

### Performance Monitoring

Integrate GPU monitoring into your rendering code:

```rust
// In your render loop
let start_time = std::time::Instant::now();

// ... rendering operations ...

let end_time = std::time::Instant::now();
let frame_time_ms = (end_time - start_time).as_secs_f32() * 1000.0;

let render_stats = RenderStats {
    frame_number: current_frame,
    gpu_frame_total_time_ms: frame_time_ms,
    // ... other stats
};

gpu_monitor.update_from_frame_stats(&render_stats);
```

## Testing Guidelines

### Unit Tests

Write comprehensive unit tests for all public APIs:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_component_creation() {
        let component = MyComponent::new();
        assert_eq!(component.value(), 0);
    }

    #[test]
    fn test_component_behavior() {
        let mut component = MyComponent::new();
        component.update(42);
        assert_eq!(component.value(), 42);
    }
}
```

### Integration Tests

Integration tests verify that components work together correctly:

```rust
#[test]
fn test_metrics_and_monitoring_integration() {
    let mut metrics = EngineMetrics::new();
    let monitor = MemoryMonitor::new("Test".to_string());
    
    // Test interaction between systems
    monitor.update_from_bytes(1024);
    let usage = monitor.get_usage_report();
    
    metrics.set_gauge("memory_usage", usage.current_bytes as f64);
    
    // Verify integration
    assert!(metrics.get_gauge_value("memory_usage").unwrap() > 0.0);
}
```

## Performance Considerations

### Memory Management

- Use `Arc` and `Mutex` judiciously for shared state
- Prefer `Rc` and `RefCell` for single-threaded scenarios
- Monitor memory usage with the built-in monitoring system

### Rendering Performance

- Minimize state changes between draw calls
- Use instancing for repeated geometry
- Monitor GPU performance with the monitoring system
- Profile regularly to identify bottlenecks

### CPU Performance

- Use profiling tools to identify hot paths
- Consider multithreading for independent operations
- Cache frequently computed values

## Error Handling

The engine uses Result types for error handling:

```rust
use khora_engine_core::subsystems::renderer::error::ResourceError;

fn create_buffer(size: usize) -> Result<BufferId, ResourceError> {
    if size == 0 {
        return Err(ResourceError::InvalidSize("Buffer size cannot be zero".to_string()));
    }
    
    // ... create buffer ...
    Ok(buffer_id)
}

// Usage
match create_buffer(1024) {
    Ok(buffer_id) => println!("Created buffer: {:?}", buffer_id),
    Err(e) => eprintln!("Failed to create buffer: {}", e),
}
```

## Contributing

### Code Style

- Follow Rust naming conventions
- Use descriptive variable and function names
- Add documentation comments for public APIs
- Run `cargo fmt` before committing

### Commit Messages

Use clear, descriptive commit messages:

```
feat: add GPU performance monitoring system
fix: resolve memory leak in texture loading
docs: update developer guide with monitoring examples
test: add integration tests for metrics system
```

### Pull Requests

- Include comprehensive tests
- Update documentation as needed
- Ensure all CI checks pass
- Provide clear description of changes

This guide provides the foundation for developing with the Khora Engine. For specific subsystems or advanced topics, refer to the dedicated documentation in the `docs/` directory.
