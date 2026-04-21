pub mod models;
pub mod parser;
pub mod renderer;

pub use models::Schematic;
pub use renderer::RenderOptions;

/// Parse a `.sch` or `.sym` file content and render it to an SVG string.
pub fn render_svg(content: &str, opts: &RenderOptions) -> Result<String, String> {
    let schematic = parser::parse(content)?;
    Ok(renderer::render_to_svg(&schematic, opts))
}
