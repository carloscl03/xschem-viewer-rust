pub mod models;
pub mod netlist;
pub mod parser;
pub mod renderer;
pub mod scene;

pub use models::{BoundingBox, DrawElement, ResolvedScene, Schematic};
pub use netlist::{extract_netlist, Instance, Net, Netlist, Pin};
pub use renderer::{dark_theme, light_theme, Renderer, RenderOptions, RenderResult};
pub use scene::SceneBuilder;

/// One-shot: parse + render a SVG en una sola llamada.
/// Para múltiples archivos del mismo PDK, usa `Renderer::new()` para compartir el caché de símbolos.
pub fn render_svg(content: &str, opts: RenderOptions) -> Result<RenderResult, String> {
    Renderer::new(opts).render(content)
}

/// One-shot: parse + resolver a `ResolvedScene` sin producir SVG.
/// Útil para backends alternativos (egui, análisis, diff estructural).
pub fn resolve_scene(content: &str, opts: &RenderOptions) -> Result<ResolvedScene, String> {
    Renderer::new(RenderOptions {
        colors: opts.colors.clone(),
        symbol_paths: opts.symbol_paths.clone(),
    })
    .resolve(content)
}
