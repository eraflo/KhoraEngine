
#[derive(Debug)]
pub struct SceneRenderData<'a> {
    _marker: std::marker::PhantomData<&'a ()>,
}

/// Trait defining the responsibilities of a rendering subsystem.
/// This acts as the interface contract for any renderer implementation.
pub trait Renderer {
    /// Renders a single frame based on the provided scene data.
    /// This will be called once per frame within the main engine loop.
    fn render(&mut self, scene_data: SceneRenderData);
}