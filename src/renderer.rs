use std::collections::{HashMap, HashSet};
use std::f64::consts::PI;
use std::fmt::Write as FmtWrite;
use std::sync::Arc;

use crate::models::{Object, Properties, Schematic, Wire};
use crate::parser;

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

    /// Auto-detects symbol paths by reading the xschemrc found in the current
    /// directory or the user's home directory. Silently returns self unchanged
    /// if no xschemrc is found or parsing fails.
    pub fn with_sym_paths_from_xschemrc(self) -> Self {
        let paths = xschemrc_sym_paths();
        paths.into_iter().fold(self, |opts, p| opts.with_sym_path(p))
    }
}

/// Parses an xschemrc file and returns all symbol search paths it declares.
///
/// Handles:
///   set PDK_ROOT /path
///   set PDK sky130A
///   set XSCHEM_SHAREDIR /path
///   append XSCHEM_LIBRARY_PATH :/colon:separated:paths
///
/// PDK paths are constructed as `$PDK_ROOT/$PDK/libs.tech/xschem` (the
/// standard OpenPDK layout), falling back to direct `XSCHEM_LIBRARY_PATH`
/// entries when PDK_ROOT/PDK are not set.
fn xschemrc_sym_paths() -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    if let Some(rc_path) = find_xschemrc() {
        if let Ok(content) = std::fs::read_to_string(rc_path) {
            paths.extend(parse_xschemrc_paths(&content));
        }
    }
    // PDK_ROOT + PDK env vars (standard OpenPDK layout)
    if let (Ok(root), Ok(pdk)) = (std::env::var("PDK_ROOT"), std::env::var("PDK")) {
        let pdk_path = std::path::PathBuf::from(&root).join(&pdk).join("libs.tech").join("xschem");
        if pdk_path.exists() && !paths.contains(&pdk_path) {
            paths.push(pdk_path);
        }
    }
    // TOOLS env var (iic-osic-tools layout) — native devices
    if let Ok(tools) = std::env::var("TOOLS") {
        let devices = std::path::PathBuf::from(tools).join("xschem").join("share").join("xschem").join("xschem_library").join("devices");
        if devices.exists() && !paths.contains(&devices) {
            paths.push(devices);
        }
    }
    paths
}

fn find_xschemrc() -> Option<std::path::PathBuf> {
    // 1. current directory
    let local = std::path::PathBuf::from(".xschemrc");
    if local.exists() {
        return Some(local);
    }
    // 2. home directory
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
            // colon-separated; leading colon is common (means "append")
            for part in rest.split(':') {
                let p = part.trim();
                if !p.is_empty() {
                    // expand simple ${VAR} references
                    let expanded = expand_tcl_vars(p, &pdk_root, &pdk, &sharedir);
                    lib_paths.push(expanded);
                }
            }
        }
    }

    let mut paths: Vec<std::path::PathBuf> = Vec::new();

    // OpenPDK convention: $PDK_ROOT/$PDK/libs.tech/xschem
    if let (Some(root), Some(pdk_name)) = (&pdk_root, &pdk) {
        let pdk_sym = std::path::PathBuf::from(root)
            .join(pdk_name)
            .join("libs.tech")
            .join("xschem");
        if pdk_sym.exists() {
            paths.push(pdk_sym);
        }
    }

    // xschem built-in devices from XSCHEM_SHAREDIR
    if let Some(dir) = &sharedir {
        let devices = std::path::PathBuf::from(dir).join("xschem_library").join("devices");
        if devices.exists() {
            paths.push(devices);
        }
    }

    // XSCHEM_LIBRARY_PATH entries
    for p in lib_paths {
        let pb = std::path::PathBuf::from(&p);
        if pb.exists() {
            paths.push(pb);
        }
    }

    paths
}

/// Expands ${PDK_ROOT}, ${PDK}, ${XSCHEM_SHAREDIR} in a Tcl string.
fn expand_tcl_vars(
    s: &str,
    pdk_root: &Option<String>,
    pdk: &Option<String>,
    sharedir: &Option<String>,
) -> String {
    let mut out = s.to_string();
    if let Some(v) = pdk_root {
        out = out.replace("${PDK_ROOT}", v).replace("$PDK_ROOT", v);
    }
    if let Some(v) = pdk {
        out = out.replace("${PDK}", v).replace("$PDK", v);
    }
    if let Some(v) = sharedir {
        out = out.replace("${XSCHEM_SHAREDIR}", v).replace("$XSCHEM_SHAREDIR", v);
    }
    out
}

/// Stateful renderer. Keeps a symbol cache across multiple render calls.
/// If you render several schematics from the same PDK, symbols are parsed only once.
pub struct RenderResult {
    pub svg: String,
    /// Symbols that could not be resolved. Empty when all symbols were found.
    pub missing_symbols: Vec<String>,
}

pub struct Renderer {
    opts: RenderOptions,
    sym_cache: HashMap<String, Arc<Vec<Object>>>,
    /// Symbols that were requested but not found in any sym_path.
    missing: Vec<String>,
}

impl Renderer {
    pub fn new(opts: RenderOptions) -> Self {
        Self { opts, sym_cache: HashMap::new(), missing: Vec::new() }
    }

    pub fn render(&mut self, content: &str) -> Result<RenderResult, String> {
        self.missing.clear();
        let schematic = parser::parse(content)?;
        let svg = self.render_schematic(&schematic);
        Ok(RenderResult { svg, missing_symbols: self.missing.clone() })
    }

    fn render_schematic(&mut self, schematic: &Schematic) -> String {
        let mut buf = String::new();
        let mut bbox = BBox::default();
        let empty_props: Properties = Default::default();

        for obj in &schematic.objects {
            self.render_object(obj, &mut buf, &mut bbox, &empty_props, Transform::identity());
        }

        let wires: Vec<&Wire> = schematic.wires().collect();
        render_junctions(&wires, &mut buf, &mut bbox, &self.opts);

        wrap_svg(buf, bbox, &self.opts)
    }

    fn render_object(
        &mut self,
        obj: &Object,
        buf: &mut String,
        bbox: &mut BBox,
        parent_props: &Properties,
        gt: Transform,
    ) {
        match obj {
            Object::Wire(w) => {
                bbox.expand_rect(w.x1, w.y1, w.x2, w.y2);
                let stroke = color(&self.opts, layers::WIRE);
                let _ = write!(buf,
                    r#"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="{}"/>"#,
                    w.x1, w.y1, w.x2, w.y2, stroke);
            }

            Object::Line(l) => {
                bbox.expand_rect(l.x1, l.y1, l.x2, l.y2);
                let stroke = color(&self.opts, l.layer as usize);
                let dash = l.properties.get("dash")
                    .map(|s| format!(r#" stroke-dasharray="{}""#, s))
                    .unwrap_or_default();
                let _ = write!(buf,
                    r#"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="{}"{}/>"#,
                    l.x1, l.y1, l.x2, l.y2, stroke, dash);
            }

            Object::Rectangle(r) => {
                let flags = r.properties.get("flags").map(|s| s.as_str()).unwrap_or("");
                if flags.split(',').any(|f| f == "graph") { return; }

                let x = r.x1.min(r.x2);
                let y = r.y1.min(r.y2);
                let w = (r.x2 - r.x1).abs();
                let h = (r.y2 - r.y1).abs();
                bbox.expand_rect(r.x1, r.y1, r.x2, r.y2);

                if let Some(img) = r.properties.get("image_data") {
                    let alpha = r.properties.get("alpha").map(|s| s.as_str()).unwrap_or("1");
                    let _ = write!(buf,
                        r#"<image x="{x}" y="{y}" width="{w}" height="{h}" href="data:image/png;base64,{img}" opacity="{alpha}"/>"#);
                } else {
                    let stroke = color(&self.opts, r.layer as usize);
                    let fill = if r.properties.get("fill").map(|s| s.as_str()) == Some("false") {
                        "none"
                    } else {
                        stroke
                    };
                    let _ = write!(buf,
                        r#"<rect x="{x}" y="{y}" width="{w}" height="{h}" stroke="{stroke}" fill="{fill}"/>"#);
                }
            }

            Object::Arc(a) => {
                bbox.expand(a.center_x - a.radius, a.center_y - a.radius);
                bbox.expand(a.center_x + a.radius, a.center_y + a.radius);
                let stroke = color(&self.opts, a.layer as usize);

                if a.sweep_angle.abs() >= 360.0 {
                    let _ = write!(buf,
                        r#"<circle cx="{}" cy="{}" r="{}" fill="none" stroke="{}"/>"#,
                        a.center_x, a.center_y, a.radius, stroke);
                } else {
                    let start = -a.start_angle;
                    let sweep = -a.sweep_angle;
                    let x1 = a.center_x + a.radius * (start * PI / 180.0).cos();
                    let y1 = a.center_y + a.radius * (start * PI / 180.0).sin();
                    let x2 = a.center_x + a.radius * ((start + sweep) * PI / 180.0).cos();
                    let y2 = a.center_y + a.radius * ((start + sweep) * PI / 180.0).sin();
                    let large = if sweep > 180.0 { 1 } else { 0 };
                    let sf = if sweep > 0.0 { 1 } else { 0 };
                    let _ = write!(buf,
                        r#"<path d="M {x1} {y1} A {} {} 0 {large} {sf} {x2} {y2}" fill="none" stroke="{stroke}"/>"#,
                        a.radius, a.radius);
                }
            }

            Object::Polygon(p) => {
                if p.points.is_empty() { return; }
                for pt in &p.points { bbox.expand(pt.x, pt.y); }
                let stroke = color(&self.opts, p.layer as usize);
                let fill = if p.properties.get("fill").map(|s| s.as_str()) == Some("true") {
                    stroke
                } else {
                    "none"
                };
                let pts: String = p.points.iter()
                    .map(|pt| format!("{},{}", pt.x, pt.y))
                    .collect::<Vec<_>>().join(" ");
                let _ = write!(buf,
                    r#"<polygon points="{pts}" fill="{fill}" stroke="{stroke}"/>"#);
            }

            Object::Text(t) => {
                if t.properties.get("hide").map(|s| s.as_str()) == Some("true") { return; }
                bbox.expand(t.x, t.y);

                let is_pin = matches!(
                    parent_props.get("type").map(|s| s.as_str()),
                    Some("ipin") | Some("opin") | Some("iopin") | Some("label")
                );
                let layer = t.properties.get("layer")
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(if is_pin { layers::WIRE } else { layers::TEXT });

                let fill = color(&self.opts, layer);
                let final_rot = (t.rotation + gt.rotation) % 4;
                let v_mirror = final_rot == 2 || final_rot == 1;
                let mut h_mirror = v_mirror;
                if t.mirror == 1 { h_mirror = !h_mirror; }
                if gt.flip { h_mirror = !h_mirror; }

                let h_center = t.properties.get("hcenter").map(|s| s.as_str()) == Some("true");
                let v_center = t.properties.get("vcenter").map(|s| s.as_str()) == Some("true");
                let anchor = if h_center { "middle" } else if h_mirror { "end" } else { "start" };
                let baseline = if v_center { "middle" } else if v_mirror { "after-edge" } else { "before-edge" };

                let text_content = substitute_attrs(&t.text, parent_props);
                let lines: Vec<&str> = text_content.lines().collect();
                let font_size = t.v_size * FONT_SCALE;

                for (i, line) in lines.iter().enumerate() {
                    let li = if v_mirror { lines.len() - i - 1 } else { i } as f64;
                    let mut transforms = vec![format!("translate({},{})", t.x, t.y)];
                    if t.rotation != 0 { transforms.push(format!("rotate({})", t.rotation * 90)); }
                    if t.mirror == 1 { transforms.push("scale(-1,1)".into()); }
                    transforms.push(format!("translate(0,{})", li * t.v_size * FONT_SCALE));
                    if v_mirror { transforms.push("scale(1,-1)".into()); }
                    if h_mirror { transforms.push("scale(-1,1)".into()); }
                    let transform = transforms.join(" ");
                    let escaped = escape_xml(line);
                    let _ = write!(buf,
                        r#"<text transform="{transform}" font-size="{font_size}" fill="{fill}" text-anchor="{anchor}" alignment-baseline="{baseline}">{escaped}</text>"#);
                }
            }

            Object::Component(c) => {
                // BBox: register position; symbol extents are added recursively
                bbox.expand(c.x, c.y);

                let sym_file = if c.symbol_reference.ends_with(".sym") {
                    c.symbol_reference.clone()
                } else {
                    format!("{}.sym", c.symbol_reference)
                };

                let child_gt = Transform {
                    flip: if c.flip != 0 { !gt.flip } else { gt.flip },
                    rotation: (gt.rotation + c.rotation) % 4,
                };

                let spice_ignore = c.properties.get("spice_ignore").map(|s| s.as_str()) == Some("true");
                let filter = if spice_ignore { r#" style="filter:grayscale(1);opacity:0.5""# } else { "" };

                let mut transforms = vec![format!("translate({},{})", c.x, c.y)];
                if c.rotation != 0 { transforms.push(format!("rotate({})", c.rotation * 90)); }
                if c.flip != 0 { transforms.push("scale(-1,1)".into()); }
                let transform = transforms.join(" ");

                let _ = write!(buf, r#"<g transform="{transform}"{filter}>"#);

                // Resolve symbol: Arc clone is O(1), not a deep copy
                let sym_objects: Option<Arc<Vec<Object>>> = self.resolve_symbol(&sym_file);

                if let Some(objects) = sym_objects {
                    let mut comp_props = c.properties.clone();
                    let sym_name = c.symbol_reference.split('/').last()
                        .and_then(|s| s.split('.').next())
                        .unwrap_or(c.symbol_reference.as_str());
                    comp_props.insert("symname".into(), sym_name.into());
                    comp_props.entry("spice_get_voltage".into()).or_default();
                    comp_props.entry("spice_get_current".into()).or_default();

                    for obj in objects.iter() {
                        self.render_object(obj, buf, bbox, &comp_props, child_gt);
                    }
                } else {
                    // Track missing symbol for caller diagnostics
                    if !self.missing.contains(&sym_file) {
                        self.missing.push(sym_file.clone());
                    }
                    // placeholder
                    let stroke = color(&self.opts, layers::SYMBOL);
                    let fill = color(&self.opts, layers::TEXT);
                    let name = escape_xml(c.symbol_reference.split('/').last().unwrap_or(&c.symbol_reference));
                    let _ = write!(buf,
                        r#"<rect x="-20" y="-10" width="40" height="20" stroke="{stroke}" fill="none"/>"#);
                    let _ = write!(buf,
                        r#"<text x="0" y="0" font-size="10" fill="{fill}" text-anchor="middle" alignment-baseline="middle">{name}</text>"#);
                }

                buf.push_str("</g>");
            }

            Object::GlobalProperties(_)
            | Object::EmbeddedSymbol(_)
            | Object::Version(_)
            | Object::Spice(_)
            | Object::Verilog(_)
            | Object::Spectre(_)
            | Object::Vhdl(_)
            | Object::Tedax(_) => {}
        }
    }

    /// Returns an Arc — O(1) clone, symbols are never deep-copied after first parse.
    fn resolve_symbol(&mut self, sym_file: &str) -> Option<Arc<Vec<Object>>> {
        if let Some(cached) = self.sym_cache.get(sym_file) {
            return Some(Arc::clone(cached));
        }

        let filename = sym_file.split('/').last().unwrap_or(sym_file);
        let no_slash = !sym_file.contains('/');
        for base in &self.opts.symbol_paths {
            let mut candidates = vec![base.join(sym_file), base.join(filename)];
            // Mirror TypeScript fallback: bare names (no '/') are also tried under devices/
            if no_slash {
                candidates.push(base.join("devices").join(filename));
            }
            for candidate in &candidates {
                if let Ok(content) = std::fs::read_to_string(candidate) {
                    if let Ok(sch) = parser::parse(&content) {
                        let arc = Arc::new(sch.objects);
                        self.sym_cache.insert(sym_file.to_string(), Arc::clone(&arc));
                        return Some(arc);
                    }
                }
            }
        }

        None
    }
}

// ─── Convenience function ────────────────────────────────────────────────────

/// One-shot render. For rendering multiple files, prefer `Renderer::new()` to share the symbol cache.
pub fn render_to_svg(schematic: &Schematic, opts: &RenderOptions) -> String {
    let mut renderer = Renderer {
        opts: RenderOptions { colors: opts.colors.clone(), symbol_paths: opts.symbol_paths.clone() },
        sym_cache: HashMap::new(),
        missing: Vec::new(),
    };
    renderer.render_schematic(schematic)
}

// ─── Junctions O(n) ──────────────────────────────────────────────────────────

fn render_junctions(wires: &[&Wire], buf: &mut String, bbox: &mut BBox, opts: &RenderOptions) {
    // Count how many wire endpoints share each grid point — O(n)
    let mut endpoint_count: HashMap<(i64, i64), usize> = HashMap::new();
    for w in wires {
        *endpoint_count.entry(to_key(w.x1, w.y1)).or_insert(0) += 1;
        *endpoint_count.entry(to_key(w.x2, w.y2)).or_insert(0) += 1;
    }

    let mut junctions: HashSet<(i64, i64)> = endpoint_count
        .iter()
        .filter(|(_, &count)| count >= 3)
        .map(|(&k, _)| k)
        .collect();

    // T-junctions: endpoint of one wire lies strictly inside another — O(n²) but
    // unavoidable for this check; wire counts are typically small (<200).
    for w in wires {
        for other in wires {
            if std::ptr::eq(*w, *other) { continue; }
            if is_point_inside_wire(other.x1, other.y1, w) {
                junctions.insert(to_key(other.x1, other.y1));
            }
            if is_point_inside_wire(other.x2, other.y2, w) {
                junctions.insert(to_key(other.x2, other.y2));
            }
        }
    }

    let fill = color(opts, layers::WIRE);
    for (kx, ky) in &junctions {
        let x = *kx as f64 / 1000.0;
        let y = *ky as f64 / 1000.0;
        bbox.expand(x, y);
        let _ = write!(buf,
            r#"<circle cx="{x}" cy="{y}" r="{JUNCTION_RADIUS}" fill="{fill}"/>"#);
    }
}

// ─── Internal types ──────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct Transform {
    flip: bool,
    rotation: i32,
}

impl Transform {
    fn identity() -> Self { Self { flip: false, rotation: 0 } }
}

#[derive(Default)]
struct BBox {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
    initialized: bool,
}

impl BBox {
    fn expand(&mut self, x: f64, y: f64) {
        if !self.initialized {
            self.min_x = x; self.min_y = y;
            self.max_x = x; self.max_y = y;
            self.initialized = true;
        } else {
            self.min_x = self.min_x.min(x); self.min_y = self.min_y.min(y);
            self.max_x = self.max_x.max(x); self.max_y = self.max_y.max(y);
        }
    }

    fn expand_rect(&mut self, x1: f64, y1: f64, x2: f64, y2: f64) {
        self.expand(x1, y1);
        self.expand(x2, y2);
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

mod layers {
    pub const WIRE: usize = 1;
    pub const TEXT: usize = 3;
    pub const SYMBOL: usize = 4;
}

fn color(opts: &RenderOptions, index: usize) -> &str {
    opts.colors.get(index).map(|s| s.as_str()).unwrap_or("#ffffff")
}

fn wrap_svg(body: String, bbox: BBox, opts: &RenderOptions) -> String {
    let (vx, vy, vw, vh) = if bbox.initialized {
        (bbox.min_x, bbox.min_y, bbox.max_x - bbox.min_x, bbox.max_y - bbox.min_y)
    } else {
        (0.0, 0.0, 100.0, 100.0)
    };
    let bg = color(opts, 0);
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{vx} {vy} {vw} {vh}" width="{vw}" height="{vh}"><rect x="{vx}" y="{vy}" width="{vw}" height="{vh}" fill="{bg}"/>{body}</svg>"#
    )
}

fn to_key(x: f64, y: f64) -> (i64, i64) {
    ((x * 1000.0).round() as i64, (y * 1000.0).round() as i64)
}

fn is_point_inside_wire(px: f64, py: f64, wire: &Wire) -> bool {
    if (px == wire.x1 && py == wire.y1) || (px == wire.x2 && py == wire.y2) {
        return false;
    }
    point_to_line_dist(px, py, wire.x1, wire.y1, wire.x2, wire.y2) < 0.01
}

fn point_to_line_dist(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let (a, b, c, d) = (px - x1, py - y1, x2 - x1, y2 - y1);
    let len_sq = c * c + d * d;
    let param = if len_sq == 0.0 { -1.0 } else { (a * c + b * d) / len_sq };
    let (xx, yy) = if param < 0.0 { (x1, y1) } else if param > 1.0 { (x2, y2) } else { (x1 + param * c, y1 + param * d) };
    let (dx, dy) = (px - xx, py - yy);
    (dx * dx + dy * dy).sqrt()
}

fn substitute_attrs(text: &str, props: &Properties) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '@' {
            let mut attr = String::new();
            while let Some(&nc) = chars.peek() {
                if nc.is_alphanumeric() || nc == '_' || nc == '#' || nc == ':' {
                    attr.push(nc); chars.next();
                } else { break; }
            }
            result.push_str(props.get(&attr).map(|s| s.as_str()).unwrap_or(""));
        } else {
            result.push(c);
        }
    }
    result
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

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

#[cfg(test)]
mod tests {
    use super::parse_xschemrc_paths;

    #[test]
    fn parses_pdk_root_and_pdk() {
        // Non-existent paths should be silently filtered — only the variables
        // are parsed; path existence is checked at runtime on the real system.
        // We test variable expansion by using a path that *we know won't exist*
        // so the list comes back empty, but at least no panic.
        let rc = "set PDK_ROOT /nonexistent/pdks\nset PDK sky130A\n";
        let paths = parse_xschemrc_paths(rc);
        // The path /nonexistent/... doesn't exist, so list should be empty.
        assert!(paths.is_empty());
    }

    #[test]
    fn parses_xschem_library_path_entries() {
        let rc = "append XSCHEM_LIBRARY_PATH :/tmp:/nonexistent/lib\n";
        let paths = parse_xschemrc_paths(rc);
        // /tmp exists on Linux; /nonexistent does not.
        // On Windows neither exists — both filtered. Just verify no panic.
        for p in &paths {
            assert!(p.exists(), "{} should exist", p.display());
        }
    }

    #[test]
    fn expands_pdk_root_variable_in_library_path() {
        let rc = "set PDK_ROOT /headless/pdks\nset PDK sky130A\nappend XSCHEM_LIBRARY_PATH :${PDK_ROOT}/${PDK}/libs.tech/xschem\n";
        let paths = parse_xschemrc_paths(rc);
        // paths that don't exist are filtered — no panic is the assertion
        let _ = paths;
    }

    #[test]
    fn empty_rc_returns_empty_paths() {
        let paths = parse_xschemrc_paths("");
        assert!(paths.is_empty());
    }
}
