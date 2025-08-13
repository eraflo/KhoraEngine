# Event System Documentation

The event system in KhoraEngine provides a decoupled, publish-subscribe architecture for communication between engine components. It enables loose coupling and flexible event-driven interactions throughout the engine.

## Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Core Components](#core-components)
3. [Event Types](#event-types)
4. [Event Bus](#event-bus)
5. [Usage Patterns](#usage-patterns)
6. [Performance Considerations](#performance-considerations)
7. [Integration Examples](#integration-examples)

## Architecture Overview

The event system follows a centralized event bus pattern:

```
┌─────────────┐    Events    ┌─────────────┐    Events    ┌─────────────┐
│  Subsystem  │──────────────▶│  Event Bus  │──────────────▶│  Subsystem  │
│   (Input)   │               │             │               │ (Renderer)  │
└─────────────┘               └─────────────┘               └─────────────┘
       ▲                             │                             │
       │                             │ Events                      │
       │                             ▼                             │
       │                      ┌─────────────┐                      │
       └──────────────────────│   Engine    │──────────────────────┘
                              │    Core     │
                              └─────────────┘
```

### Design Principles

1. **Loose Coupling**: Components communicate without direct dependencies
2. **Type Safety**: Strong typing for all events
3. **Performance**: Minimal overhead for event dispatch
4. **Extensibility**: Easy to add new event types

## Core Components

### EngineEvent Enum

The main event enumeration defines all possible events in the system:

```rust
#[derive(Debug, Clone)]
pub enum EngineEvent {
    /// Input event from the user
    Input(InputEvent),
    
    /// Render completion with statistics
    Render(RenderStats),
    
    /// Window events (resize, close, etc.)
    Window(WindowEvent),
    
    /// System resource events
    Resource(ResourceEvent),
    
    /// Custom events for extensibility
    Custom(Box<dyn CustomEvent>),
}
```

### Event Bus

The event bus manages event distribution:

```rust
pub struct EventBus {
    events: VecDeque<EngineEvent>,
    subscribers: HashMap<EventType, Vec<Box<dyn EventHandler>>>,
}

impl EventBus {
    pub fn new() -> Self { /* ... */ }
    pub fn publish(&mut self, event: EngineEvent) { /* ... */ }
    pub fn poll(&mut self) -> Option<EngineEvent> { /* ... */ }
    pub fn subscribe(&mut self, event_type: EventType, handler: Box<dyn EventHandler>) { /* ... */ }
}
```

## Event Types

### Input Events

Input events represent user interactions:

```rust
#[derive(Debug, Clone)]
pub enum InputEvent {
    Keyboard {
        key: KeyCode,
        state: ElementState,
        modifiers: ModifiersState,
    },
    Mouse {
        button: MouseButton,
        state: ElementState,
        position: Vec2,
    },
    MouseMotion {
        position: Vec2,
        delta: Vec2,
    },
    MouseWheel {
        delta: MouseScrollDelta,
    },
    WindowFocused(bool),
}
```

Usage example:
```rust
match event {
    EngineEvent::Input(InputEvent::Keyboard { key, state, .. }) => {
        if key == KeyCode::Escape && state == ElementState::Pressed {
            // Handle escape key
        }
    }
    EngineEvent::Input(InputEvent::Mouse { button, state, position }) => {
        if button == MouseButton::Left && state == ElementState::Pressed {
            // Handle left click at position
        }
    }
    _ => {}
}
```

### Window Events

Window events represent window system interactions:

```rust
#[derive(Debug, Clone)]
pub enum WindowEvent {
    Resized(Extent2D),
    CloseRequested,
    Focused(bool),
    Minimized(bool),
    ScaleFactorChanged(f64),
}
```

### Resource Events

Resource events indicate resource management activities:

```rust
#[derive(Debug, Clone)]
pub enum ResourceEvent {
    MemoryPressure { usage_mb: f32, budget_mb: f32 },
    VramPressure { usage_mb: f32, budget_mb: f32 },
    AssetLoaded { asset_id: String, size_bytes: usize },
    AssetUnloaded { asset_id: String },
    ShaderCompiled { shader_id: ShaderModuleId, compile_time_ms: f32 },
}
```

### Custom Events

The system supports custom events through a trait:

```rust
pub trait CustomEvent: std::fmt::Debug + Send + Sync {
    fn event_type(&self) -> &'static str;
    fn as_any(&self) -> &dyn std::any::Any;
}

// Example custom event
#[derive(Debug)]
pub struct AudioEvent {
    pub sound_id: u32,
    pub event_type: AudioEventType,
}

#[derive(Debug)]
pub enum AudioEventType {
    Started,
    Finished,
    Failed(String),
}

impl CustomEvent for AudioEvent {
    fn event_type(&self) -> &'static str { "audio" }
    fn as_any(&self) -> &dyn std::any::Any { self }
}
```

## Event Bus

### Implementation

The event bus uses a simple queue-based approach for immediate event processing:

```rust
impl EventBus {
    pub fn publish(&mut self, event: EngineEvent) {
        self.events.push_back(event);
    }
    
    pub fn poll(&mut self) -> Option<EngineEvent> {
        self.events.pop_front()
    }
    
    pub fn poll_all(&mut self) -> Vec<EngineEvent> {
        let mut events = Vec::new();
        while let Some(event) = self.poll() {
            events.push(event);
        }
        events
    }
    
    pub fn clear(&mut self) {
        self.events.clear();
    }
}
```

### Thread Safety

The current implementation is single-threaded but designed to be thread-safe. For multi-threaded scenarios, consider using appropriate synchronization primitives around the event bus.
        while events.is_empty() {
            events = self.event_notify.wait(events).unwrap();
        }
        events.pop_front().unwrap()
    }
}
```

## Usage Patterns

### Engine Integration

The engine processes events in its main loop:

```rust
impl Engine {
    pub fn update(&mut self) -> Result<(), EngineError> {
        // Process window events from winit
        self.handle_window_events();
        
        // Process engine events
        let events = self.event_bus.poll_all();
        for event in events {
            self.handle_event(event)?;
        }
        
        Ok(())
    }
    
    fn handle_event(&mut self, event: EngineEvent) -> Result<(), EngineError> {
        match event {
            EngineEvent::Input(input_event) => {
                self.handle_input_event(input_event)?;
            }
            EngineEvent::Window(window_event) => {
                self.handle_window_event(window_event)?;
            }
            EngineEvent::Render(render_stats) => {
                self.handle_render_completion(render_stats);
            }
            EngineEvent::Resource(resource_event) => {
                self.handle_resource_event(resource_event)?;
            }
            EngineEvent::Custom(custom_event) => {
                self.handle_custom_event(custom_event)?;
            }
        }
        Ok(())
    }
}
```

### Subsystem Communication

Subsystems can publish events to communicate with other parts of the engine:

```rust
impl RenderSystem {
    pub fn render(&mut self, objects: &[RenderObject]) -> Result<(), RenderError> {
        let start_time = Instant::now();
        
        // Perform rendering...
        
        let render_time = start_time.elapsed();
        
        // Publish render completion event
        let stats = RenderStats {
            frame_number: self.frame_count,
            cpu_render_time_ms: render_time.as_secs_f32() * 1000.0,
            gpu_frame_time_ms: self.gpu_timer.last_frame_time(),
            vram_usage_mb: self.device.vram_usage_mb(),
            // ... other stats
        };
        
        self.event_bus.publish(EngineEvent::Render(stats));
        Ok(())
    }
}
```

### Event-Driven State Management

Events can drive state changes throughout the engine:

```rust
struct GameState {
    is_paused: bool,
    current_level: Option<String>,
    player_health: f32,
}

impl GameState {
    fn handle_event(&mut self, event: &EngineEvent) {
        match event {
            EngineEvent::Input(InputEvent::Keyboard { key: KeyCode::Space, state: ElementState::Pressed, .. }) => {
                self.is_paused = !self.is_paused;
                log::info!("Game {}", if self.is_paused { "paused" } else { "resumed" });
            }
            EngineEvent::Custom(custom_event) => {
                if let Some(game_event) = custom_event.as_any().downcast_ref::<GameEvent>() {
                    match game_event {
                        GameEvent::PlayerDamaged(damage) => {
                            self.player_health -= damage;
                            if self.player_health <= 0.0 {
                                // Publish game over event
                            }
                        }
                        GameEvent::LevelCompleted(level) => {
                            self.current_level = Some(level.clone());
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
```

## Performance Considerations

### Event Processing Cost

- **Memory**: Events are cloned when published, keep event data lightweight
- **Processing**: Use pattern matching efficiently, avoid expensive operations in handlers
- **Batching**: Process events in batches rather than one-by-one when possible

### Optimization Strategies

```rust
// ✅ Good: Lightweight event data
#[derive(Debug, Clone)]
pub struct QuickEvent {
    pub event_id: u32,
    pub timestamp: Instant,
}

// ❌ Avoid: Heavy event data
#[derive(Debug, Clone)]
pub struct HeavyEvent {
    pub large_data: Vec<u8>, // This gets cloned on every publish!
    pub complex_state: HashMap<String, ComplexStruct>,
}

// ✅ Better: Use references or handles
#[derive(Debug, Clone)]
pub struct EfficientEvent {
    pub data_handle: DataHandle,
    pub metadata: SmallMetadata,
}
```

### Memory Management

```rust
impl EventBus {
    pub fn update(&mut self) {
        // Limit event queue size to prevent memory bloat
        const MAX_EVENTS: usize = 1000;
        
        if self.events.len() > MAX_EVENTS {
            log::warn!("Event queue overflow, dropping {} old events", 
                      self.events.len() - MAX_EVENTS);
            self.events.drain(0..self.events.len() - MAX_EVENTS);
        }
    }
}
```

## Integration Examples

### Input System Integration

```rust
impl InputSystem {
    pub fn process_winit_event(&mut self, event: &winit::event::Event<()>) {
        match event {
            winit::event::Event::DeviceEvent { event, .. } => {
                match event {
                    winit::event::DeviceEvent::Key(keyboard_input) => {
                        let input_event = InputEvent::Keyboard {
                            key: keyboard_input.virtual_keycode.unwrap_or(KeyCode::Unknown),
                            state: keyboard_input.state,
                            modifiers: self.modifiers_state,
                        };
                        self.event_bus.publish(EngineEvent::Input(input_event));
                    }
                    winit::event::DeviceEvent::MouseMotion { delta } => {
                        let input_event = InputEvent::MouseMotion {
                            position: self.mouse_position,
                            delta: Vec2::new(delta.0 as f32, delta.1 as f32),
                        };
                        self.event_bus.publish(EngineEvent::Input(input_event));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}
```

### Resource Management Integration

```rust
impl AssetManager {
    pub fn load_asset(&mut self, path: &str) -> Result<AssetHandle, AssetError> {
        let start_time = Instant::now();
        
        // Load asset...
        let asset_data = std::fs::read(path)?;
        let handle = self.store_asset(asset_data);
        
        let load_time = start_time.elapsed();
        
        // Publish asset loaded event
        let event = ResourceEvent::AssetLoaded {
            asset_id: path.to_string(),
            size_bytes: asset_data.len(),
        };
        self.event_bus.publish(EngineEvent::Resource(event));
        
        log::info!("Loaded asset {} ({} bytes) in {:.2}ms", 
                   path, asset_data.len(), load_time.as_secs_f32() * 1000.0);
        
        Ok(handle)
    }
}
```

### Performance Monitoring Integration

```rust
impl PerformanceMonitor {
    pub fn handle_render_event(&mut self, stats: &RenderStats) {
        self.frame_times.push(stats.gpu_frame_time_ms);
        
        // Keep only recent frames
        if self.frame_times.len() > 300 { // 5 seconds at 60fps
            self.frame_times.remove(0);
        }
        
        // Check for performance issues
        let avg_frame_time = self.frame_times.iter().sum::<f32>() / self.frame_times.len() as f32;
        
        if avg_frame_time > 16.67 { // 60fps threshold
            let event = ResourceEvent::PerformanceWarning {
                metric: "frame_time".to_string(),
                current_value: avg_frame_time,
                threshold: 16.67,
            };
            self.event_bus.publish(EngineEvent::Resource(event));
        }
    }
}
```

This event system provides a solid foundation for decoupled communication within KhoraEngine, enabling flexible architectures and extensible event-driven patterns.

For implementation details, see the source code in `khora_engine_core/src/event/`.
