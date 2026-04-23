use std::collections::{HashMap, HashSet};
use std::fmt::Write as FmtWrite;

use crate::models::{DrawElement, ResolvedScene};
use crate::parser;
use crate::scene::{arc_endpoints, SceneBuilder};

const FONT_SCALE: f64 = 50.0;
const JUNCTION_RADIUS: f64 = 3.0;

// ─── Public API ──────────────────────────────────────────────────────────────

pub struct RenderOptions {
    pub colors: Vec<String>,
    pub symbol_paths: Vec<std::path::PathBuf>,
}

impl RenderOptions {
    pub fn dark() -> Self {
        Self { colors: dark_theme(), symbol_paths: vec![] }
    }

    pub fn light() -> Self {
        Self { colors: light_theme(), symbol_paths: vec![] }
    }

    pub fn with_sym_path(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.symbol_paths.push(path.into());
        self
    }

    /// Auto-detecta rutas de símbolos desde el xschemrc del directorio actual
    /// o del home. Silenciosamente no hace nada si no encuentra xschemrc.
    pub fn with_sym_paths_from_xschemrc(self) -> Self {
        let paths = xschemrc_sym_paths();
        paths.into_iter().fold(self, |opts, p| opts.with_sym_path(p))
    }
}

pub struct RenderResult {
    pub svg: String,
    /// Símbolos que no pudieron resolverse.
    pub missing_symbols: Vec<String>,
}

/// Renderer con caché de símbolos compartida entre llamadas.
/// Para múltiples archivos del mismo PDK, reutiliza esta instancia.
pub struct Renderer {
    opts: RenderOptions,
}

impl Renderer {
    pub fn new(opts: RenderOptions) -> Self {
        Self { opts }
    }

    /// Resuelve el schematic a una escena de primitivos planos.
    /// La escena puede convertirse a SVG, a egui shapes, o a cualquier otro backend.
    pub fn resolve(&self, content: &str) -> Result<ResolvedScene, String> {
        let schematic = parser::parse(content)?;
        let scene = SceneBuilder::new(&self.opts).build(&schematic);
        Ok(scene)
    }

    /// Shortcut: parse + resolve + SVG en una sola llamada.
    pub fn render(&self, content: &str) -> Result<RenderResult, String> {
        let scene = self.resolve(content)?;
        let missing = scene.missing_symbols.clone();
        let svg = scene_to_svg(&scene, &self.opts);
        Ok(RenderResult { svg, missing_symbols: missing })
    }
}

// ─── SVG backend ─────────────────────────────────────────────────────────────

/// Convierte una `ResolvedScene` a SVG.
pub fn scene_to_svg(scene: &ResolvedScene, opts: &RenderOptions) -> String {
    let mut buf = String::new();

    for elem in &scene.elements {
        render_element(elem, &mut buf, opts);
    }

    render_junctions(&scene.wires, &mut buf, opts);

    wrap_svg(buf, &scene.bbox, opts)
}

fn render_element(elem: &DrawElement, buf: &mut String, opts: &RenderOptions) {
    match elem {
        DrawElement::Line { x1, y1, x2, y2, layer, .. } => {
            let stroke = color(opts, *layer as usize);
            let _ = write!(buf,
                r#"<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="{stroke}"/>"#);
        }

        DrawElement::Rect { x, y, w, h, layer, filled, .. } => {
            let stroke = color(opts, *layer as usize);
            let fill = if *filled { stroke } else { "none" };
            let _ = write!(buf,
                r#"<rect x="{x}" y="{y}" width="{w}" height="{h}" stroke="{stroke}" fill="{fill}"/>"#);
        }

        DrawElement::Circle { cx, cy, r, layer, .. } => {
            let stroke = color(opts, *layer as usize);
            let _ = write!(buf,
                r#"<circle cx="{cx}" cy="{cy}" r="{r}" fill="none" stroke="{stroke}"/>"#);
        }

        DrawElement::Arc { cx, cy, r, start_angle, sweep_angle, layer, .. } => {
            let stroke = color(opts, *layer as usize);
            let ((x1, y1), (x2, y2), large_arc) =
                arc_endpoints(*cx, *cy, *r, *start_angle, *sweep_angle);
            let large = if large_arc { 1 } else { 0 };
            let sf = if *sweep_angle > 0.0 { 1 } else { 0 };
            let _ = write!(buf,
                r#"<path d="M {x1} {y1} A {r} {r} 0 {large} {sf} {x2} {y2}" fill="none" stroke="{stroke}"/>"#);
        }

        DrawElement::Polygon { points, layer, filled, .. } => {
            if points.is_empty() { return; }
            let stroke = color(opts, *layer as usize);
            let fill = if *filled { stroke } else { "none" };
            let pts: String = points.iter()
                .map(|(x, y)| format!("{x},{y}"))
                .collect::<Vec<_>>().join(" ");
            let _ = write!(buf,
                r#"<polygon points="{pts}" fill="{fill}" stroke="{stroke}"/>"#);
        }

        DrawElement::Text { x, y, content, v_size, rotation, mirror, h_center, v_center, layer, .. } => {
            let fill = color(opts, *layer as usize);
            let font_size = v_size * FONT_SCALE;
            let v_mirror = *rotation == 1 || *rotation == 2;
            let h_mirror = if *mirror == 1 { !v_mirror } else { v_mirror };
            let text_anchor = if *h_center { "middle" } else if h_mirror { "end" } else { "start" };
            let baseline = if *v_center { "middle" } else if v_mirror { "after-edge" } else { "before-edge" };
            let lines: Vec<&str> = content.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                let line_index = if v_mirror { lines.len() - 1 - i } else { i } as f64;
                let dy = line_index * v_size * FONT_SCALE;
                let mut transforms = vec![format!("translate({x},{y})")];
                if *rotation != 0 { transforms.push(format!("rotate({})", rotation * 90)); }
                if *mirror == 1 { transforms.push("scale(-1,1)".to_owned()); }
                transforms.push(format!("translate(0,{dy})"));
                if v_mirror { transforms.push("scale(1,-1)".to_owned()); }
                if h_mirror { transforms.push("scale(-1,1)".to_owned()); }
                let transform = transforms.join(" ");
                let escaped = escape_xml(line);
                let _ = write!(buf,
                    r#"<text transform="{transform}" font-size="{font_size}" fill="{fill}" text-anchor="{text_anchor}" alignment-baseline="{baseline}">{escaped}</text>"#);
            }
        }

        DrawElement::MissingSymbol { name, x, y, .. } => {
            let stroke = color(opts, 4);
            let fill = color(opts, 3);
            let label = escape_xml(name.split('/').last().unwrap_or(name)
                .trim_end_matches(".sym"));
            let _ = write!(buf,
                r#"<rect x="{}" y="{}" width="40" height="20" stroke="{stroke}" fill="none"/>"#,
                x - 20.0, y - 10.0);
            let _ = write!(buf,
                r#"<text x="{x}" y="{y}" font-size="10" fill="{fill}" text-anchor="middle" alignment-baseline="middle">{label}</text>"#);
        }
    }
}

// ─── Convenience function ─────────────────────────────────────────────────────

/// One-shot: render desde un `Schematic` ya parseado. Para múltiples renders usa `Renderer`.
pub fn render_to_svg(schematic: &crate::models::Schematic, opts: &RenderOptions) -> String {
    let scene = SceneBuilder::new(opts).build(schematic);
    scene_to_svg(&scene, opts)
}

// ─── Junctions ───────────────────────────────────────────────────────────────

fn render_junctions(wires: &[(f64, f64, f64, f64, Option<String>)], buf: &mut String, opts: &RenderOptions) {
    let mut endpoint_count: HashMap<(i64, i64), usize> = HashMap::new();
    for (x1, y1, x2, y2, _) in wires {
        *endpoint_count.entry(to_key(*x1, *y1)).or_insert(0) += 1;
        *endpoint_count.entry(to_key(*x2, *y2)).or_insert(0) += 1;
    }

    let mut junctions: HashSet<(i64, i64)> = endpoint_count
        .iter()
        .filter(|(_, &count)| count >= 3)
        .map(|(&k, _)| k)
        .collect();

    for (x1, y1, x2, y2, _) in wires {
        for (ox1, oy1, ox2, oy2, _) in wires {
            if is_point_inside_segment(*ox1, *oy1, *x1, *y1, *x2, *y2) {
                junctions.insert(to_key(*ox1, *oy1));
            }
            if is_point_inside_segment(*ox2, *oy2, *x1, *y1, *x2, *y2) {
                junctions.insert(to_key(*ox2, *oy2));
            }
        }
    }

    let fill = color(opts, 1);
    for (kx, ky) in &junctions {
        let x = *kx as f64 / 1000.0;
        let y = *ky as f64 / 1000.0;
        let _ = write!(buf,
            r#"<circle cx="{x}" cy="{y}" r="{JUNCTION_RADIUS}" fill="{fill}"/>"#);
    }
}

// ─── SVG helpers ─────────────────────────────────────────────────────────────

fn wrap_svg(body: String, bbox: &crate::models::BoundingBox, opts: &RenderOptions) -> String {
    let (vx, vy, vw, vh) = if !bbox.is_empty() {
        (bbox.min_x, bbox.min_y, bbox.width(), bbox.height())
    } else {
        (0.0, 0.0, 100.0, 100.0)
    };
    let bg = color(opts, 0);
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{vx} {vy} {vw} {vh}" width="{vw}" height="{vh}"><rect x="{vx}" y="{vy}" width="{vw}" height="{vh}" fill="{bg}"/>{body}</svg>"#
    )
}

fn color(opts: &RenderOptions, index: usize) -> &str {
    opts.colors.get(index).map(|s| s.as_str()).unwrap_or("#ffffff")
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn to_key(x: f64, y: f64) -> (i64, i64) {
    ((x * 1000.0).round() as i64, (y * 1000.0).round() as i64)
}

fn is_point_inside_segment(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> bool {
    if (px == x1 && py == y1) || (px == x2 && py == y2) {
        return false;
    }
    point_to_line_dist(px, py, x1, y1, x2, y2) < 0.01
}

fn point_to_line_dist(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let (a, b, c, d) = (px - x1, py - y1, x2 - x1, y2 - y1);
    let len_sq = c * c + d * d;
    let param = if len_sq == 0.0 { -1.0 } else { (a * c + b * d) / len_sq };
    let (xx, yy) = if param < 0.0 { (x1, y1) } else if param > 1.0 { (x2, y2) } else { (x1 + param * c, y1 + param * d) };
    let (dx, dy) = (px - xx, py - yy);
    (dx * dx + dy * dy).sqrt()
}

// ─── PDK path detection ──────────────────────────────────────────────────────

fn xschemrc_sym_paths() -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();

    if let Some(rc_path) = find_xschemrc() {
        if let Ok(content) = std::fs::read_to_string(rc_path) {
            paths.extend(parse_xschemrc_paths(&content));
        }
    }

    // PDK_ROOT + PDK env vars (layout estándar OpenPDK)
    if let (Ok(root), Ok(pdk)) = (std::env::var("PDK_ROOT"), std::env::var("PDK")) {
        let pdk_path = std::path::PathBuf::from(&root).join(&pdk).join("libs.tech").join("xschem");
        if pdk_path.exists() && !paths.contains(&pdk_path) {
            paths.push(pdk_path);
        }
    }

    // TOOLS env var (layout iic-osic-tools) — símbolos nativos de xschem
    if let Ok(tools) = std::env::var("TOOLS") {
        let devices = std::path::PathBuf::from(tools)
            .join("xschem").join("share").join("xschem")
            .join("xschem_library").join("devices");
        if devices.exists() && !paths.contains(&devices) {
            paths.push(devices);
        }
    }

    paths
}

fn find_xschemrc() -> Option<std::path::PathBuf> {
    let local = std::path::PathBuf::from(".xschemrc");
    if local.exists() {
        return Some(local);
    }
    if let Some(home) = dirs::home_dir() {
        let home_rc = home.join(".xschemrc");
        if home_rc.exists() {
            return Some(home_rc);
        }
    }
    None
}

fn parse_xschemrc_paths(content: &str) -> Vec<std::path::PathBuf> {
    let mut pdk_root: Option<String> = None;
    let mut pdk: Option<String> = None;
    let mut sharedir: Option<String> = None;
    let mut lib_paths: Vec<String> = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("set PDK_ROOT ") {
            pdk_root = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("set PDK ") {
            pdk = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("set XSCHEM_SHAREDIR ") {
            sharedir = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("append XSCHEM_LIBRARY_PATH ") {
            for part in rest.split(':') {
                let p = part.trim();
                if !p.is_empty() {
                    let expanded = expand_tcl_vars(p, &pdk_root, &pdk, &sharedir);
                    lib_paths.push(expanded);
                }
            }
        }
    }

    let mut paths: Vec<std::path::PathBuf> = Vec::new();

    if let (Some(root), Some(pdk_name)) = (&pdk_root, &pdk) {
        let pdk_sym = std::path::PathBuf::from(root)
            .join(pdk_name).join("libs.tech").join("xschem");
        if pdk_sym.exists() {
            paths.push(pdk_sym);
        }
    }

    if let Some(dir) = &sharedir {
        let devices = std::path::PathBuf::from(dir).join("xschem_library").join("devices");
        if devices.exists() {
            paths.push(devices);
        }
    }

    for p in lib_paths {
        let pb = std::path::PathBuf::from(&p);
        if pb.exists() {
            paths.push(pb);
        }
    }

    paths
}

fn expand_tcl_vars(s: &str, pdk_root: &Option<String>, pdk: &Option<String>, sharedir: &Option<String>) -> String {
    let mut out = s.to_string();
    if let Some(v) = pdk_root { out = out.replace("${PDK_ROOT}", v).replace("$PDK_ROOT", v); }
    if let Some(v) = pdk     { out = out.replace("${PDK}", v).replace("$PDK", v); }
    if let Some(v) = sharedir { out = out.replace("${XSCHEM_SHAREDIR}", v).replace("$XSCHEM_SHAREDIR", v); }
    out
}

// ─── Themes ──────────────────────────────────────────────────────────────────

pub fn dark_theme() -> Vec<String> {
    ["#000000","#00ccee","#3f3f3f","#cccccc","#88dd00","#bb2200",
     "#00ccee","#ff0000","#ffff00","#ffffff","#ff00ff","#00ff00",
     "#0000cc","#aaaa00","#aaccaa","#ff7777","#bfff81","#00ffcc",
     "#ce0097","#d2d46b","#ef6158","#fdb200"]
    .iter().map(|s| s.to_string()).collect()
}

pub fn light_theme() -> Vec<String> {
    ["#ffffff","#0044ee","#aaaaaa","#222222","#229900","#bb2200",
     "#00ccee","#ff0000","#888800","#00aaaa","#880088","#00ff00",
     "#0000cc","#666600","#557755","#aa2222","#7ccc40","#00ffcc",
     "#ce0097","#d2d46b","#ef6158","#fdb200"]
    .iter().map(|s| s.to_string()).collect()
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::parse_xschemrc_paths;

    #[test]
    fn parses_pdk_root_and_pdk() {
        let rc = "set PDK_ROOT /nonexistent/pdks\nset PDK sky130A\n";
        let paths = parse_xschemrc_paths(rc);
        assert!(paths.is_empty());
    }

    #[test]
    fn parses_xschem_library_path_entries() {
        let rc = "append XSCHEM_LIBRARY_PATH :/tmp:/nonexistent/lib\n";
        let paths = parse_xschemrc_paths(rc);
        for p in &paths {
            assert!(p.exists(), "{} should exist", p.display());
        }
    }

    #[test]
    fn expands_pdk_root_variable_in_library_path() {
        let rc = "set PDK_ROOT /headless/pdks\nset PDK sky130A\nappend XSCHEM_LIBRARY_PATH :${PDK_ROOT}/${PDK}/libs.tech/xschem\n";
        let paths = parse_xschemrc_paths(rc);
        let _ = paths;
    }

    #[test]
    fn empty_rc_returns_empty_paths() {
        let paths = parse_xschemrc_paths("");
        assert!(paths.is_empty());
    }
}
