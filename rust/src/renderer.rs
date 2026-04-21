use std::collections::HashMap;
use std::f64::consts::PI;
use std::fmt::Write as FmtWrite;

use crate::models::{Object, Properties, Schematic, Wire};

const FONT_SCALE: f64 = 50.0;
const JUNCTION_RADIUS: f64 = 3.0;

pub struct RenderOptions {
    pub colors: Vec<String>,
    /// Ordered list of directories to search for .sym files.
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
}

#[derive(Clone, Copy)]
struct Transform {
    flip: bool,
    rotation: i32, // 0-3
}

impl Transform {
    fn identity() -> Self {
        Self { flip: false, rotation: 0 }
    }
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
            self.min_x = x;
            self.min_y = y;
            self.max_x = x;
            self.max_y = y;
            self.initialized = true;
        } else {
            self.min_x = self.min_x.min(x);
            self.min_y = self.min_y.min(y);
            self.max_x = self.max_x.max(x);
            self.max_y = self.max_y.max(y);
        }
    }

    fn expand_rect(&mut self, x1: f64, y1: f64, x2: f64, y2: f64) {
        self.expand(x1, y1);
        self.expand(x2, y2);
    }
}

pub fn render_to_svg(schematic: &Schematic, opts: &RenderOptions) -> String {
    let mut body = String::new();
    let mut bbox = BBox::default();
    let props: Properties = Default::default();
    let mut sym_cache: HashMap<String, Vec<Object>> = HashMap::new();

    for obj in &schematic.objects {
        render_object(obj, &mut body, &mut bbox, &props, Transform::identity(), opts, &mut sym_cache);
    }

    // junctions
    let wires: Vec<&Wire> = schematic.wires().collect();
    render_junctions(&wires, &mut body, &mut bbox, opts);

    let (vx, vy, vw, vh) = if bbox.initialized {
        (bbox.min_x, bbox.min_y, bbox.max_x - bbox.min_x, bbox.max_y - bbox.min_y)
    } else {
        (0.0, 0.0, 100.0, 100.0)
    };

    let bg = opts.colors.first().map(|s| s.as_str()).unwrap_or("#000000");

    let mut svg = String::new();
    let _ = write!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{vx} {vy} {vw} {vh}" width="{vw}" height="{vh}">"#
    );
    let _ = write!(
        svg,
        r#"<rect x="{vx}" y="{vy}" width="{vw}" height="{vh}" fill="{bg}"/>"#
    );
    svg.push_str(&body);
    svg.push_str("</svg>");
    svg
}

fn color(opts: &RenderOptions, index: usize) -> &str {
    opts.colors.get(index).map(|s| s.as_str()).unwrap_or("#ffffff")
}

// Layer indices matching the TypeScript Layers enum
mod layers {
    pub const BACKGROUND: usize = 0;
    pub const WIRE: usize = 1;
    pub const GRID: usize = 2;
    pub const TEXT: usize = 3;
    pub const SYMBOL: usize = 4;
    pub const PIN: usize = 5;
}

fn render_object(
    obj: &Object,
    out: &mut String,
    bbox: &mut BBox,
    parent_props: &Properties,
    global_transform: Transform,
    opts: &RenderOptions,
    sym_cache: &mut HashMap<String, Vec<Object>>,
) {
    match obj {
        Object::Wire(w) => {
            bbox.expand_rect(w.x1, w.y1, w.x2, w.y2);
            let stroke = color(opts, layers::WIRE);
            let _ = write!(
                out,
                r#"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="{}"/>"#,
                w.x1, w.y1, w.x2, w.y2, stroke
            );
        }

        Object::Line(l) => {
            bbox.expand_rect(l.x1, l.y1, l.x2, l.y2);
            let stroke = color(opts, l.layer as usize);
            let dash = l.properties.get("dash").map(|s| format!(r#" stroke-dasharray="{}""#, s)).unwrap_or_default();
            let _ = write!(
                out,
                r#"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="{}"{}/>"#,
                l.x1, l.y1, l.x2, l.y2, stroke, dash
            );
        }

        Object::Rectangle(r) => {
            let flags: Vec<&str> = r.properties.get("flags").map(|s| s.as_str()).unwrap_or("").split(',').collect();
            if flags.contains(&"graph") {
                return;
            }
            let x = r.x1.min(r.x2);
            let y = r.y1.min(r.y2);
            let w = (r.x2 - r.x1).abs();
            let h = (r.y2 - r.y1).abs();
            bbox.expand_rect(r.x1, r.y1, r.x2, r.y2);

            if let Some(image_data) = r.properties.get("image_data") {
                let alpha = r.properties.get("alpha").map(|s| s.as_str()).unwrap_or("1");
                let _ = write!(
                    out,
                    r#"<image x="{x}" y="{y}" width="{w}" height="{h}" href="data:image/png;base64,{image_data}" opacity="{alpha}"/>"#
                );
            } else {
                let stroke = color(opts, r.layer as usize);
                let fill = if r.properties.get("fill").map(|s| s.as_str()) == Some("false") {
                    "none".to_string()
                } else {
                    stroke.to_string()
                };
                let _ = write!(
                    out,
                    r#"<rect x="{x}" y="{y}" width="{w}" height="{h}" stroke="{stroke}" fill="{fill}"/>"#
                );
            }
        }

        Object::Arc(a) => {
            bbox.expand(a.center_x - a.radius, a.center_y - a.radius);
            bbox.expand(a.center_x + a.radius, a.center_y + a.radius);
            let stroke = color(opts, a.layer as usize);

            if a.sweep_angle.abs() >= 360.0 {
                let _ = write!(
                    out,
                    r#"<circle cx="{}" cy="{}" r="{}" fill="none" stroke="{}"/>"#,
                    a.center_x, a.center_y, a.radius, stroke
                );
            } else {
                let start = -a.start_angle;
                let sweep = -a.sweep_angle;
                let x1 = a.center_x + a.radius * (start * PI / 180.0).cos();
                let y1 = a.center_y + a.radius * (start * PI / 180.0).sin();
                let x2 = a.center_x + a.radius * ((start + sweep) * PI / 180.0).cos();
                let y2 = a.center_y + a.radius * ((start + sweep) * PI / 180.0).sin();
                let large = if sweep > 180.0 { 1 } else { 0 };
                let sweep_flag = if sweep > 0.0 { 1 } else { 0 };
                let _ = write!(
                    out,
                    r#"<path d="M {x1} {y1} A {} {} 0 {large} {sweep_flag} {x2} {y2}" fill="none" stroke="{stroke}"/>"#,
                    a.radius, a.radius
                );
            }
        }

        Object::Polygon(p) => {
            if p.points.is_empty() {
                return;
            }
            for pt in &p.points {
                bbox.expand(pt.x, pt.y);
            }
            let stroke = color(opts, p.layer as usize);
            let fill = if p.properties.get("fill").map(|s| s.as_str()) == Some("true") {
                stroke.to_string()
            } else {
                "none".to_string()
            };
            let pts: String = p.points.iter().map(|pt| format!("{},{}", pt.x, pt.y)).collect::<Vec<_>>().join(" ");
            let _ = write!(out, r#"<polygon points="{pts}" fill="{fill}" stroke="{stroke}"/>"#);
        }

        Object::Text(t) => {
            if t.properties.get("hide").map(|s| s.as_str()) == Some("true") {
                return;
            }
            bbox.expand(t.x, t.y);

            let is_pin = matches!(
                parent_props.get("type").map(|s| s.as_str()),
                Some("ipin") | Some("opin") | Some("iopin") | Some("label")
            );
            let layer = t.properties.get("layer")
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(if is_pin { layers::WIRE } else { layers::TEXT });

            let fill = color(opts, layer);
            let final_rotation = (t.rotation + global_transform.rotation) % 4;
            let v_mirror = final_rotation == 2 || final_rotation == 1;
            let mut h_mirror = v_mirror;
            if t.mirror == 1 { h_mirror = !h_mirror; }
            if global_transform.flip { h_mirror = !h_mirror; }

            let h_center = t.properties.get("hcenter").map(|s| s.as_str()) == Some("true");
            let v_center = t.properties.get("vcenter").map(|s| s.as_str()) == Some("true");

            let text_anchor = if h_center { "middle" } else if h_mirror { "end" } else { "start" };
            let baseline = if v_center { "middle" } else if v_mirror { "after-edge" } else { "before-edge" };

            // attribute substitution: @attr → parent property value
            let text_content = substitute_attrs(&t.text, parent_props);

            let lines: Vec<&str> = text_content.lines().collect();
            let font_size = t.v_size * FONT_SCALE;

            for (i, line) in lines.iter().enumerate() {
                let line_index = if v_mirror { lines.len() - i - 1 } else { i } as f64;
                let mut transforms = vec![format!("translate({},{})", t.x, t.y)];
                if t.rotation != 0 {
                    transforms.push(format!("rotate({})", t.rotation * 90));
                }
                if t.mirror == 1 {
                    transforms.push("scale(-1,1)".to_string());
                }
                transforms.push(format!("translate(0,{})", line_index * t.v_size * FONT_SCALE));
                if v_mirror { transforms.push("scale(1,-1)".to_string()); }
                if h_mirror { transforms.push("scale(-1,1)".to_string()); }

                let transform = transforms.join(" ");
                let escaped = escape_xml(line);
                let _ = write!(
                    out,
                    r#"<text transform="{transform}" font-size="{font_size}" fill="{fill}" text-anchor="{text_anchor}" alignment-baseline="{baseline}">{escaped}</text>"#
                );
            }
        }

        Object::Component(c) => {
            bbox.expand(c.x, c.y);
            let mut transforms = vec![format!("translate({},{})", c.x, c.y)];
            if c.rotation != 0 {
                transforms.push(format!("rotate({})", c.rotation * 90));
            }
            if c.flip != 0 {
                transforms.push("scale(-1,1)".to_string());
            }
            let transform = transforms.join(" ");

            let child_transform = Transform {
                flip: if c.flip != 0 { !global_transform.flip } else { global_transform.flip },
                rotation: (global_transform.rotation + c.rotation) % 4,
            };

            let sym_ref = &c.symbol_reference;
            let sym_file = if sym_ref.ends_with(".sym") {
                sym_ref.clone()
            } else {
                format!("{}.sym", sym_ref)
            };

            let spice_ignore = c.properties.get("spice_ignore").map(|s| s.as_str()) == Some("true");
            let filter = if spice_ignore { r#" style="filter:grayscale(1);opacity:0.5""# } else { "" };

            let _ = write!(out, r#"<g transform="{transform}"{filter}>"#);

            // Try to resolve symbol from disk — clone to release the borrow on sym_cache
            let resolved: Option<Vec<Object>> = resolve_symbol(&sym_file, opts, sym_cache).cloned();
            if let Some(sym_objects) = resolved {
                let mut comp_props = c.properties.clone();
                let symbol_name = sym_ref.split('/').last()
                    .and_then(|s| s.split('.').next())
                    .unwrap_or(sym_ref.as_str());
                comp_props.insert("symname".to_string(), symbol_name.to_string());

                for sub_obj in &sym_objects {
                    render_object(sub_obj, out, bbox, &comp_props, child_transform, opts, sym_cache);
                }
            } else {
                // placeholder box with symbol name
                let symbol_name = sym_ref.split('/').last().unwrap_or(sym_ref.as_str());
                let stroke = color(opts, layers::SYMBOL);
                let fill = color(opts, layers::TEXT);
                let _ = write!(
                    out,
                    r#"<rect x="-20" y="-10" width="40" height="20" stroke="{stroke}" fill="none"/>"#
                );
                let _ = write!(
                    out,
                    r#"<text x="0" y="0" font-size="10" fill="{fill}" text-anchor="middle" alignment-baseline="middle">{}</text>"#,
                    escape_xml(symbol_name)
                );
            }

            out.push_str("</g>");
        }

        Object::GlobalProperties(props) => {
            // propagate into parent_props is handled at the call site if needed
            let _ = props;
        }

        Object::EmbeddedSymbol(objects) => {
            for obj in objects {
                render_object(obj, out, bbox, parent_props, global_transform, opts, sym_cache);
            }
        }

        Object::Version(_)
        | Object::Spice(_)
        | Object::Verilog(_)
        | Object::Spectre(_)
        | Object::Vhdl(_)
        | Object::Tedax(_) => {}
    }
}

fn resolve_symbol<'a>(
    sym_file: &str,
    opts: &RenderOptions,
    cache: &'a mut HashMap<String, Vec<Object>>,
) -> Option<&'a Vec<Object>> {
    if cache.contains_key(sym_file) {
        return cache.get(sym_file);
    }

    if opts.symbol_paths.is_empty() {
        return None;
    }

    // For each search path, try: base/sym_file and base/filename.sym
    let filename = sym_file.split('/').last().unwrap_or(sym_file);
    for base in &opts.symbol_paths {
        let candidates = [base.join(sym_file), base.join(filename)];
        for candidate in &candidates {
            if let Ok(content) = std::fs::read_to_string(candidate) {
                if let Ok(sch) = crate::parser::parse(&content) {
                    cache.insert(sym_file.to_string(), sch.objects);
                    return cache.get(sym_file);
                }
            }
        }
    }

    None
}

fn render_junctions(wires: &[&Wire], out: &mut String, bbox: &mut BBox, opts: &RenderOptions) {
    use std::collections::HashMap;

    let mut endpoints: HashMap<(i64, i64), usize> = HashMap::new();
    for w in wires {
        *endpoints.entry(to_key(w.x1, w.y1)).or_insert(0) += 1;
        *endpoints.entry(to_key(w.x2, w.y2)).or_insert(0) += 1;
    }

    let mut junctions = std::collections::HashSet::new();
    for (key, count) in &endpoints {
        if *count >= 3 {
            junctions.insert(*key);
        }
    }

    // point in middle of another wire
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
        let _ = write!(
            out,
            r#"<circle cx="{x}" cy="{y}" r="{JUNCTION_RADIUS}" fill="{fill}"/>"#
        );
    }
}

fn to_key(x: f64, y: f64) -> (i64, i64) {
    ((x * 1000.0).round() as i64, (y * 1000.0).round() as i64)
}

fn is_point_inside_wire(px: f64, py: f64, wire: &Wire) -> bool {
    if (px == wire.x1 && py == wire.y1) || (px == wire.x2 && py == wire.y2) {
        return false;
    }
    point_to_line_distance(px, py, wire.x1, wire.y1, wire.x2, wire.y2) < 0.01
}

fn point_to_line_distance(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let a = px - x1;
    let b = py - y1;
    let c = x2 - x1;
    let d = y2 - y1;
    let len_sq = c * c + d * d;
    let param = if len_sq == 0.0 { -1.0 } else { (a * c + b * d) / len_sq };
    let (xx, yy) = if param < 0.0 {
        (x1, y1)
    } else if param > 1.0 {
        (x2, y2)
    } else {
        (x1 + param * c, y1 + param * d)
    };
    let dx = px - xx;
    let dy = py - yy;
    (dx * dx + dy * dy).sqrt()
}

fn substitute_attrs(text: &str, props: &Properties) -> String {
    // Replace @attr with the property value, or leave as-is
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '@' {
            let mut attr = String::new();
            while let Some(&nc) = chars.peek() {
                if nc.is_alphanumeric() || nc == '_' || nc == '#' || nc == ':' {
                    attr.push(nc);
                    chars.next();
                } else {
                    break;
                }
            }
            if let Some(val) = props.get(&attr) {
                result.push_str(val);
            } else {
                result.push('@');
                result.push_str(&attr);
            }
        } else {
            result.push(c);
        }
    }
    result
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn dark_theme() -> Vec<String> {
    [
        "#000000", "#00ccee", "#3f3f3f", "#cccccc", "#88dd00", "#bb2200",
        "#00ccee", "#ff0000", "#ffff00", "#ffffff", "#ff00ff", "#00ff00",
        "#0000cc", "#aaaa00", "#aaccaa", "#ff7777", "#bfff81", "#00ffcc",
        "#ce0097", "#d2d46b", "#ef6158", "#fdb200",
    ]
    .iter().map(|s| s.to_string()).collect()
}

pub fn light_theme() -> Vec<String> {
    [
        "#ffffff", "#0044ee", "#aaaaaa", "#222222", "#229900", "#bb2200",
        "#00ccee", "#ff0000", "#888800", "#00aaaa", "#880088", "#00ff00",
        "#0000cc", "#666600", "#557755", "#aa2222", "#7ccc40", "#00ffcc",
        "#ce0097", "#d2d46b", "#ef6158", "#fdb200",
    ]
    .iter().map(|s| s.to_string()).collect()
}
