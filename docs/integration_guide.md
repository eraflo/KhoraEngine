# Engine Integration Guide

This guide explains how to integrate new subsystems, extend functionality, and work with KhoraEngine's modular architecture. It's designed for developers who want to add new features or integrate the engine into their projects.

## Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Adding New Subsystems](#adding-new-subsystems)
3. [Extending Existing Systems](#extending-existing-systems)
4. [Event System Integration](#event-system-integration)
5. [Resource Management](#resource-management)
6. [Performance Integration](#performance-integration)
7. [Testing Integration](#testing-integration)
8. [Best Practices](#best-practices)

## Architecture Overview

KhoraEngine follows a modular, event-driven architecture:

```
┌─────────────────────────────────────────────────────────┐
│                    Application                          │
├─────────────────────────────────────────────────────────┤
│                   Engine Core                           │
│  ┌───────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │  Event Bus    │  │  Monitoring  │  │    Timer     │  │
│  └───────────────┘  └──────────────┘  └──────────────┘  │
├─────────────────────────────────────────────────────────┤
│                   Subsystems                            │
│  ┌───────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │   Renderer    │  │    Input     │  │   Custom     │  │
│  └───────────────┘  └──────────────┘  └──────────────┘  │
├─────────────────────────────────────────────────────────┤
│              Foundation Modules                         │
│  ┌───────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │     Math      │  │   Memory     │  │   Window     │  │
│  └───────────────┘  └──────────────┘  └──────────────┘  │
└─────────────────────────────────────────────────────────┘
```

### Key Principles

1. **Modularity**: Each subsystem is independent and communicates via events
2. **SAA Readiness**: Built-in monitoring and adaptive capabilities
3. **Type Safety**: Strong typing throughout the API
4. **Performance**: Minimal overhead in hot paths
5. **Extensibility**: Easy to add new subsystems and functionality

## Adding New Subsystems

### Step 1: Define the Subsystem Interface

Create a new subsystem by defining its core structure and interface:

```rust
// khora_engine_core/src/subsystems/audio.rs
use crate::event::EngineEvent;
use crate::core::monitoring::ResourceMonitor;
use std::collections::HashMap;

pub struct AudioSystem {
    // Internal state
    sounds: HashMap<SoundId, Sound>,
    playing_sounds: Vec<PlayingSound>,
    master_volume: f32,
    
    // Performance tracking
    cpu_time_budget_ms: f32,
    last_frame_time_ms: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SoundId(pub usize);

impl AudioSystem {
    pub fn new() -> Self {
        Self {
            sounds: HashMap::new(),
            playing_sounds: Vec::new(),
            master_volume: 1.0,
            cpu_time_budget_ms: 2.0, // 2ms budget per frame
            last_frame_time_ms: 0.0,
        }
    }
    
    pub fn load_sound(&mut self, path: &str) -> Result<SoundId, AudioError> {
        // Implementation
    }
    
    pub fn play_sound(&mut self, sound_id: SoundId) -> Result<(), AudioError> {
        // Implementation
    }
    
    pub fn update(&mut self) -> Vec<EngineEvent> {
        let start_time = std::time::Instant::now();
        
        // Update playing sounds
        self.update_playing_sounds();
        
        // Track performance
        self.last_frame_time_ms = start_time.elapsed().as_secs_f32() * 1000.0;
        
        // Return events (if any)
        Vec::new()
    }
}
```

### Step 2: Implement ResourceMonitor

Make your subsystem SAA-ready by implementing resource monitoring:

```rust
impl ResourceMonitor for AudioSystem {
    fn get_resource_usage(&self) -> HashMap<String, f64> {
        let mut usage = HashMap::new();
        
        usage.insert("audio_cpu_time_ms".to_string(), self.last_frame_time_ms as f64);
        usage.insert("audio_playing_sounds".to_string(), self.playing_sounds.len() as f64);
        usage.insert("audio_loaded_sounds".to_string(), self.sounds.len() as f64);
        usage.insert("audio_master_volume".to_string(), self.master_volume as f64);
        
        usage
    }
    
    fn get_resource_limits(&self) -> HashMap<String, f64> {
        let mut limits = HashMap::new();
        limits.insert("audio_cpu_time_ms".to_string(), self.cpu_time_budget_ms as f64);
        limits
    }
    
    fn is_healthy(&self) -> bool {
        self.last_frame_time_ms <= self.cpu_time_budget_ms
    }
}
```

### Step 3: Define Events

Define events that your subsystem can produce or consume:

```rust
#[derive(Debug, Clone)]
pub enum AudioEvent {
    SoundLoaded { sound_id: SoundId, path: String, duration_ms: f32 },
    SoundStarted { sound_id: SoundId },
    SoundFinished { sound_id: SoundId },
    VolumeChanged { new_volume: f32 },
    CpuBudgetExceeded { actual_ms: f32, budget_ms: f32 },
}
```

### Step 4: Integrate with Engine

Add your subsystem to the engine's main structure:

```rust
// khora_engine_core/src/core/engine.rs
impl Engine {
    pub fn new(window: Window) -> Result<Self, EngineError> {
        let mut engine = Self {
            // ... existing fields
            audio_system: AudioSystem::new(),
        };
        
        Ok(engine)
    }
    
    pub fn update(&mut self) -> Result<(), EngineError> {
        // ... existing update logic
        
        // Update audio system
        let audio_events = self.audio_system.update();
        for event in audio_events {
            self.event_bus.publish(event);
        }
        
        Ok(())
    }
}
```

### Step 5: Update Module Exports

Export your subsystem from the appropriate modules:

```rust
// khora_engine_core/src/subsystems/mod.rs
pub mod audio;
pub mod input;
pub mod renderer;

pub use audio::AudioSystem;
```

## Extending Existing Systems

### Adding New Render Features

Extend the rendering system by adding new pipeline types or render passes:

```rust
// Add a new render pass type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RenderPassType {
    Main,
    Shadow,
    PostProcess,
    UI,
    Custom(u32),
}

// Extend render pipeline descriptor
impl RenderPipelineDescriptor {
    pub fn for_shadow_mapping() -> Self {
        Self {
            label: Some("shadow_pipeline"),
            pass_type: RenderPassType::Shadow,
            // ... shadow-specific configuration
        }
    }
}
```

### Adding New Math Types

Extend the math module with new geometric types:

```rust
// khora_engine_core/src/math/shapes.rs
use super::{Vec3, Mat4};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sphere {
    pub center: Vec3,
    pub radius: f32,
}

impl Sphere {
    pub fn new(center: Vec3, radius: f32) -> Self {
        Self { center, radius }
    }
    
    pub fn contains(&self, point: Vec3) -> bool {
        self.center.distance(point) <= self.radius
    }
    
    pub fn intersects(&self, other: &Sphere) -> bool {
        self.center.distance(other.center) <= (self.radius + other.radius)
    }
}
```

### Adding New Input Types

Extend input handling for new device types:

```rust
// Extend InputEvent with new device types
#[derive(Debug, Clone)]
pub enum InputEvent {
    // ... existing variants
    Gamepad {
        id: u32,
        button: GamepadButton,
        state: ElementState,
    },
    GamepadAxis {
        id: u32,
        axis: GamepadAxis,
        value: f32,
    },
    Touch {
        id: u64,
        phase: TouchPhase,
        position: Vec2,
    },
}
```

## Event System Integration

### Publishing Events

Subsystems should publish events to communicate with other parts of the engine:

```rust
impl AudioSystem {
    fn play_sound_internal(&mut self, sound_id: SoundId) -> Result<(), AudioError> {
        // Play the sound...
        
        // Publish event
        let event = EngineEvent::Custom(Box::new(AudioEvent::SoundStarted { sound_id }));
        self.publish_event(event);
        
        Ok(())
    }
    
    fn publish_event(&self, event: EngineEvent) {
        // In practice, this would use a shared event bus reference
        // For now, events are returned from update() method
    }
}
```

### Consuming Events

Handle events from other subsystems:

```rust
impl AudioSystem {
    pub fn handle_event(&mut self, event: &EngineEvent) -> Result<(), AudioError> {
        match event {
            EngineEvent::Input(InputEvent::Keyboard { key: KeyCode::M, state: ElementState::Pressed, .. }) => {
                // Toggle mute
                self.master_volume = if self.master_volume > 0.0 { 0.0 } else { 1.0 };
            }
            EngineEvent::Custom(custom_event) => {
                // Handle custom audio events
                if let Some(audio_event) = custom_event.as_any().downcast_ref::<AudioEvent>() {
                    match audio_event {
                        AudioEvent::VolumeChanged { new_volume } => {
                            self.master_volume = *new_volume;
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}
```

## Resource Management

### Memory Management Integration

Integrate with the engine's memory tracking:

```rust
impl AudioSystem {
    fn load_sound_with_tracking(&mut self, path: &str) -> Result<SoundId, AudioError> {
        use crate::memory::get_allocation_stats;
        
        let before_stats = get_allocation_stats();
        
        // Load sound data
        let sound_data = std::fs::read(path)?;
        let sound = Sound::from_data(sound_data)?;
        
        let after_stats = get_allocation_stats();
        let memory_used = after_stats.total_allocated_bytes - before_stats.total_allocated_bytes;
        
        log::debug!("Loaded audio file '{}': {} bytes", path, memory_used);
        
        let sound_id = self.next_sound_id();
        self.sounds.insert(sound_id, sound);
        
        Ok(sound_id)
    }
}
```

### GPU Resource Integration

For subsystems that use GPU resources, integrate with the graphics device:

```rust
impl ParticleSystem {
    pub fn new(device: Arc<dyn GraphicsDevice>) -> Self {
        let vertex_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("particle_vertices"),
            size: MAX_PARTICLES * std::mem::size_of::<ParticleVertex>() as u64,
            usage: BufferUsage::VERTEX | BufferUsage::COPY_DST,
            mapped_at_creation: false,
        }).expect("Failed to create particle vertex buffer");
        
        Self {
            device,
            vertex_buffer,
            particles: Vec::new(),
            // ...
        }
    }
}
```

## Performance Integration

### CPU Performance Tracking

Use the engine's timing utilities to track subsystem performance:

```rust
use crate::core::timer::Stopwatch;

impl PhysicsSystem {
    pub fn update(&mut self, dt: f32) -> Vec<EngineEvent> {
        let mut timer = Stopwatch::new();
        
        // Time collision detection
        timer.start();
        self.detect_collisions();
        timer.stop();
        let collision_time = timer.elapsed_ms();
        
        // Time physics integration
        timer.reset();
        timer.start();
        self.integrate_physics(dt);
        timer.stop();
        let integration_time = timer.elapsed_ms();
        
        // Update performance stats
        self.last_collision_time_ms = collision_time;
        self.last_integration_time_ms = integration_time;
        
        log::trace!(
            "Physics update: collision {:.2}ms, integration {:.2}ms",
            collision_time, integration_time
        );
        
        Vec::new()
    }
}
```

### Performance Budgets

Implement performance budgets for adaptive behavior:

```rust
impl ParticleSystem {
    pub fn update_with_budget(&mut self, dt: f32, time_budget_ms: f32) -> UpdateResult {
        let start_time = std::time::Instant::now();
        let mut particles_updated = 0;
        
        for particle in &mut self.particles {
            if start_time.elapsed().as_secs_f32() * 1000.0 > time_budget_ms {
                break;
            }
            
            particle.update(dt);
            particles_updated += 1;
        }
        
        UpdateResult {
            particles_updated,
            time_used_ms: start_time.elapsed().as_secs_f32() * 1000.0,
            budget_exceeded: particles_updated < self.particles.len(),
        }
    }
}
```

## Testing Integration

### Unit Tests

Write comprehensive tests for your subsystems:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn audio_system_initialization() {
        let audio_system = AudioSystem::new();
        assert_eq!(audio_system.master_volume, 1.0);
        assert!(audio_system.sounds.is_empty());
        assert!(audio_system.playing_sounds.is_empty());
    }
    
    #[test]
    fn sound_loading_and_playback() {
        let mut audio_system = AudioSystem::new();
        
        // This would need a mock sound file
        // let sound_id = audio_system.load_sound("test_sound.wav").unwrap();
        // assert!(audio_system.sounds.contains_key(&sound_id));
        
        // let result = audio_system.play_sound(sound_id);
        // assert!(result.is_ok());
    }
    
    #[test]
    fn resource_monitoring() {
        let audio_system = AudioSystem::new();
        let usage = audio_system.get_resource_usage();
        
        assert!(usage.contains_key("audio_cpu_time_ms"));
        assert!(usage.contains_key("audio_playing_sounds"));
        assert_eq!(usage["audio_playing_sounds"], 0.0);
    }
}
```

### Integration Tests

Test integration with the engine by adding tests at the end of your subsystem files:

```rust
// At the end of khora_engine_core/src/subsystems/audio.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Engine, EngineEvent};

    #[test]
    fn audio_engine_integration() {
        // Test that audio system integrates correctly
        let audio_system = AudioSystem::new();
        assert!(audio_system.is_ok());
        
        // Test resource usage reporting
        let usage = audio_system.unwrap().get_resource_usage();
        assert!(usage.cpu_time_ms >= 0.0);
        assert!(usage.memory_bytes > 0);
    }
    
    #[test] 
    fn audio_event_handling() {
        let mut audio_system = AudioSystem::new().unwrap();
        
        // Test event handling
        let play_event = AudioEvent::Play { sound_id: 1 };
        audio_system.handle_event(play_event);
        
        // Verify the sound is tracked
        assert!(audio_system.is_sound_playing(1));
    }
}
}
```

## Best Practices

### Performance Best Practices

1. **Profile Early and Often**: Use the built-in timing utilities
2. **Respect Budgets**: Implement time and memory budgets for adaptive behavior
3. **Minimize Allocations**: Use object pools and pre-allocated collections
4. **Batch Operations**: Group similar operations together

```rust
// ✅ Good: Batch operations
impl ParticleSystem {
    fn update_particles_batched(&mut self, dt: f32) {
        // Update all positions in one pass
        for particle in &mut self.particles {
            particle.position += particle.velocity * dt;
        }
        
        // Update all physics in another pass (better cache usage)
        for particle in &mut self.particles {
            particle.velocity += particle.acceleration * dt;
        }
    }
}

// ❌ Avoid: Scattered operations
impl ParticleSystem {
    fn update_particles_scattered(&mut self, dt: f32) {
        for particle in &mut self.particles {
            particle.position += particle.velocity * dt;
            particle.velocity += particle.acceleration * dt;
            particle.update_some_other_state();
            // More scattered memory accesses...
        }
    }
}
```

### Error Handling Best Practices

1. **Use Result Types**: Prefer `Result<T, E>` over panics
2. **Define Clear Error Types**: Create specific error enums for your subsystem
3. **Provide Context**: Include helpful information in error messages

```rust
#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("Sound file not found: {path}")]
    SoundNotFound { path: String },
    
    #[error("Failed to decode audio: {details}")]
    DecodingFailed { details: String },
    
    #[error("Audio device error: {message}")]
    DeviceError { message: String },
    
    #[error("Sound {id:?} is not loaded")]
    SoundNotLoaded { id: SoundId },
}
```

### SAA Preparation Best Practices

1. **Implement ResourceMonitor**: Make all subsystems monitorable
2. **Support Multiple Strategies**: Design for different quality/performance modes
3. **Report Meaningful Metrics**: Provide actionable performance data
4. **Design for Performance**: Consider how your subsystem reports performance metrics

```rust
// Example: Performance-aware subsystem design
impl ResourceMonitor for AudioSystem {
    fn get_resource_usage(&self) -> ResourceUsage {
        ResourceUsage {
            cpu_time_ms: self.last_frame_cpu_time,
            memory_bytes: self.calculate_memory_usage(),
            custom_metrics: HashMap::from([
                ("active_sources".to_string(), self.active_sources.len() as f32),
                ("buffer_underruns".to_string(), self.buffer_underruns as f32),
            ]),
        }
    }
}

impl AudioSystem {
    pub fn update(&mut self, delta_time: f32) -> Result<(), AudioError> {
        let start_time = Instant::now();
        
        // Perform audio processing...
        self.process_audio_sources(delta_time)?;
        
        // Track performance
        self.last_frame_cpu_time = start_time.elapsed().as_secs_f32() * 1000.0;
        
        Ok(())
    }
}
```

This integration guide provides the foundation for extending KhoraEngine with new functionality while maintaining architectural principles and performance monitoring capabilities.
