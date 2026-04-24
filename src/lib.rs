pub mod error;
pub mod models;
pub mod netlist;
pub mod parser;
pub mod renderer;
pub mod scene;
pub mod semantic;
pub mod text_layout;
pub mod theme;
pub mod viewport;

pub use error::{Result, XschemError};
pub use models::{BoundingBox, DrawElement, ResolvedScene, Schematic};
pub use netlist::{extract_netlist, fill_connectivity, Instance, Net, Netlist, Pin};
pub use renderer::{dark_theme, light_theme, Renderer, RenderOptions, RenderResult};
pub use scene::SceneBuilder;
pub use semantic::{
    diff, is_xschem, parse_semantic,
    ChangeKind, ComponentDiff, DiffReport,
    SemanticComponent, SemanticSchematic, SemanticWire,
};
pub use text_layout::{resolve_text_layout, HAlign, LineDirection, TextLayout, VBaseline};
pub use theme::Theme;
pub use viewport::{world_to_screen, Viewport};

/// One-shot: parse + render a SVG en una sola llamada.
/// Para múltiples archivos del mismo PDK, usa `Renderer::new()` para compartir el caché de símbolos.
pub fn render_svg(content: &str, opts: RenderOptions) -> Result<RenderResult> {
    Renderer::new(opts).render(content)
}

/// One-shot: parse + resolver a `ResolvedScene` sin producir SVG.
/// Útil para backends alternativos (egui, análisis, diff estructural).
pub fn resolve_scene(content: &str, opts: &RenderOptions) -> Result<ResolvedScene> {
    Renderer::new(RenderOptions {
        colors: opts.colors.clone(),
        symbol_paths: opts.symbol_paths.clone(),
    })
    .resolve(content)
}
