pub mod models;
pub mod parser;
pub mod renderer;

pub use models::Schematic;
pub use renderer::{Renderer, RenderOptions};

/// One-shot: parse + render en una sola llamada.
/// Para múltiples archivos del mismo PDK, usa `Renderer::new()` para compartir el caché de símbolos.
pub fn render_svg(content: &str, opts: &RenderOptions) -> Result<String, String> {
    let schematic = parser::parse(content)?;
    Ok(renderer::render_to_svg(&schematic, opts))
}
