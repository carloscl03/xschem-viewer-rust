pub mod models;
pub mod parser;
pub mod renderer;

pub use models::Schematic;
pub use renderer::{Renderer, RenderOptions, RenderResult};

/// One-shot: parse + render en una sola llamada.
/// Para múltiples archivos del mismo PDK, usa `Renderer::new()` para compartir el caché de símbolos.
pub fn render_svg(content: &str, opts: RenderOptions) -> Result<RenderResult, String> {
    Renderer::new(opts).render(content)
}
