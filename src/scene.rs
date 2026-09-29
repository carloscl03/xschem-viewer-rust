use std::collections::HashMap;
use std::f64::consts::PI;
use std::sync::Arc;

use crate::models::{
    BoundingBox, DrawElement, Object, Properties, ResolvedScene, Schematic,
};
use crate::parser;
use crate::renderer::RenderOptions;

// ─── Transform ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Default)]
pub(crate) struct Transform {
    tx: f64,
    ty: f64,
    /// 0–3 en pasos de 90°
    rotation: i32,
    flip: bool,
}

impl Transform {
    pub(crate) fn identity() -> Self {
        Self::default()
    }

    /// Como Xschem (macro `ROTATION`): primero el espejo en X, después la
    /// rotación. En el otro orden, un símbolo rotado 90°/270° y espejado
    /// queda reflejado del lado contrario (y sus pines, cambiados de lado).
    pub(crate) fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        let fx = if self.flip { -x } else { x };
        let (rx, ry) = rotate(fx, y, self.rotation);
        (rx + self.tx, ry + self.ty)
    }

    /// Transformación de un símbolo dentro de otro: `P(R_c(F_c(q)) + t_c)`.
    /// Un espejo del padre invierte el sentido de la rotación del hijo
    /// (`F·R(θ) = R(−θ)·F`), así que en ese caso las rotaciones se restan.
    pub(crate) fn child(&self, tx: f64, ty: f64, rotation: i32, flip: bool) -> Self {
        let combined_rotation = if self.flip { self.rotation - rotation } else { self.rotation + rotation }.rem_euclid(4);
        let combined_flip = self.flip != flip;
        let (ox, oy) = self.apply(tx, ty);
        Self { tx: ox, ty: oy, rotation: combined_rotation, flip: combined_flip }
    }

    /// Ángulo de inicio de un arco transformado (grados, sentido matemático
    /// con el eje Y hacia abajo, como Xschem). El espejo refleja el tramo
    /// `[s, s+w]` a `[180−s−w, 180−s]`; cada paso de rotación resta 90°
    /// (Xschem: `270·rot + 180 − b − a` con espejo, `a + 270·rot` sin él).
    fn arc_start(&self, start: f64, sweep: f64) -> f64 {
        let s = if self.flip { 180.0 - start - sweep } else { start };
        (s - 90.0 * self.rotation as f64).rem_euclid(360.0)
    }

}

fn rotate(x: f64, y: f64, steps: i32) -> (f64, f64) {
    match ((steps % 4) + 4) % 4 {
        0 => (x, y),
        1 => (-y, x),
        2 => (-x, -y),
        3 => (y, -x),
        _ => unreachable!(),
    }
}

// ─── Scene builder ───────────────────────────────────────────────────────────

pub struct SceneBuilder<'a> {
    opts: &'a RenderOptions,
    sym_cache: HashMap<String, Arc<Vec<Object>>>,
    elements: Vec<DrawElement>,
    bbox: BoundingBox,
    missing: Vec<String>,
    wires: Vec<(f64, f64, f64, f64, Option<String>)>,
    /// Posiciones de pines en espacio mundo. Clave: nombre de instancia.
    pin_positions: HashMap<String, Vec<(String, f64, f64)>>,
}

impl<'a> SceneBuilder<'a> {
    pub fn new(opts: &'a RenderOptions) -> Self {
        Self {
            opts,
            sym_cache: HashMap::new(),
            elements: Vec::new(),
            bbox: BoundingBox::default(),
            missing: Vec::new(),
            wires: Vec::new(),
            pin_positions: HashMap::new(),
        }
    }

    pub fn build(mut self, schematic: &Schematic) -> ResolvedScene {
        self.visit_objects(&schematic.objects, Transform::identity(), &Properties::default(), None);
        ResolvedScene {
            elements: self.elements,
            bbox: self.bbox,
            missing_symbols: self.missing,
            wires: self.wires,
            pin_positions: self.pin_positions,
        }
    }

    fn visit_objects(
        &mut self,
        objects: &[Object],
        gt: Transform,
        parent_props: &Properties,
        component_id: Option<&str>,
    ) {
        for obj in objects {
            self.visit(obj, gt, parent_props, component_id);
        }
    }

    fn visit(
        &mut self,
        obj: &Object,
        gt: Transform,
        parent_props: &Properties,
        component_id: Option<&str>,
    ) {
        match obj {
            Object::Wire(w) => {
                let (x1, y1) = gt.apply(w.x1, w.y1);
                let (x2, y2) = gt.apply(w.x2, w.y2);
                self.bbox.expand_rect(x1, y1, x2, y2);
                // Solo los wires del schematic raíz participan en junction detection
                if component_id.is_none() {
                    let label = w.properties.get("lab").cloned();
                    self.wires.push((x1, y1, x2, y2, label));
                }
                self.elements.push(DrawElement::Line {
                    x1, y1, x2, y2,
                    layer: 1,
                    component_id: component_id.map(str::to_owned),
                });
            }

            Object::Line(l) => {
                let (x1, y1) = gt.apply(l.x1, l.y1);
                let (x2, y2) = gt.apply(l.x2, l.y2);
                self.bbox.expand_rect(x1, y1, x2, y2);
                self.elements.push(DrawElement::Line {
                    x1, y1, x2, y2,
                    layer: l.layer,
                    component_id: component_id.map(str::to_owned),
                });
            }

            Object::Rectangle(r) => {
                let flags = r.properties.get("flags").map(|s| s.as_str()).unwrap_or("");
                if flags.split(',').any(|f| f == "graph") {
                    return;
                }
                let (x1, y1) = gt.apply(r.x1, r.y1);
                let (x2, y2) = gt.apply(r.x2, r.y2);
                let x = x1.min(x2);
                let y = y1.min(y2);
                let w = (x2 - x1).abs();
                let h = (y2 - y1).abs();
                self.bbox.expand_rect(x1, y1, x2, y2);
                let filled = r.properties.get("fill").map(|s| s != "false").unwrap_or(true);
                self.elements.push(DrawElement::Rect {
                    x, y, w, h,
                    layer: r.layer,
                    filled,
                    component_id: component_id.map(str::to_owned),
                });
            }

            Object::Arc(a) => {
                let (cx, cy) = gt.apply(a.center_x, a.center_y);
                self.bbox.expand(cx - a.radius, cy - a.radius);
                self.bbox.expand(cx + a.radius, cy + a.radius);

                if a.sweep_angle.abs() >= 360.0 {
                    self.elements.push(DrawElement::Circle {
                        cx, cy, r: a.radius,
                        layer: a.layer,
                        component_id: component_id.map(str::to_owned),
                    });
                } else {
                    self.elements.push(DrawElement::Arc {
                        cx, cy, r: a.radius,
                        start_angle: gt.arc_start(a.start_angle, a.sweep_angle),
                        sweep_angle: a.sweep_angle,
                        layer: a.layer,
                        component_id: component_id.map(str::to_owned),
                    });
                }
            }

            Object::Polygon(p) => {
                if p.points.is_empty() {
                    return;
                }
                let points: Vec<(f64, f64)> = p.points.iter()
                    .map(|pt| gt.apply(pt.x, pt.y))
                    .collect();
                for &(x, y) in &points {
                    self.bbox.expand(x, y);
                }
                let filled = p.properties.get("fill").map(|s| s == "true").unwrap_or(false);
                self.elements.push(DrawElement::Polygon {
                    points,
                    layer: p.layer,
                    filled,
                    component_id: component_id.map(str::to_owned),
                });
            }

            Object::Text(t) => {
                if t.properties.get("hide").map(|s| s.as_str()) == Some("true") {
                    return;
                }
                let (x, y) = gt.apply(t.x, t.y);
                self.bbox.expand(x, y);
                let content = crate::tcleval::eval_text(&substitute_attrs(&t.text, parent_props));
                // Como Xschem (`draw_symbol`): con espejo, un texto de rotación
                // impar gira 180° más para seguir legible del mismo lado.
                let extra = if gt.flip && t.rotation % 2 == 1 { 2 } else { 0 };
                let combined_rotation = (t.rotation + gt.rotation + extra).rem_euclid(4);
                let layer = t.properties.get("layer")
                    .and_then(|s| s.parse::<i32>().ok())
                    .unwrap_or(3);
                let h_center = t.properties.get("hcenter").map(|s| s == "true").unwrap_or(false);
                let v_center = t.properties.get("vcenter").map(|s| s == "true").unwrap_or(false);
                let mirror = if gt.flip { 1 - t.mirror } else { t.mirror };
                self.elements.push(DrawElement::Text {
                    x, y, content,
                    v_size: t.v_size,
                    h_size: t.h_size,
                    rotation: combined_rotation,
                    mirror,
                    h_center,
                    v_center,
                    layer,
                    component_id: component_id.map(str::to_owned),
                });
            }

            Object::Component(c) => {
                self.bbox.expand(c.x, c.y);

                let sym_file = if c.symbol_reference.ends_with(".sym") {
                    c.symbol_reference.clone()
                } else {
                    format!("{}.sym", c.symbol_reference)
                };

                let sym_name = c.symbol_reference.split('/').last()
                    .and_then(|s| s.split('.').next())
                    .unwrap_or(c.symbol_reference.as_str());

                // Detección de pin: ipin/opin/iopin dentro de un símbolo padre.
                // El lab= de este componente es el nombre del pin.
                // La posición en espacio mundo es la del componente padre + offset de este sub-componente.
                if let Some(parent_instance) = component_id {
                    if matches!(sym_name, "ipin" | "opin" | "iopin") {
                        if let Some(lab) = c.properties.get("lab") {
                            if !lab.is_empty() {
                                let (px, py) = gt.apply(c.x, c.y);
                                self.pin_positions
                                    .entry(parent_instance.to_owned())
                                    .or_default()
                                    .push((lab.clone(), px, py));
                            }
                        }
                        // Los pines no necesitan recursión — son símbolos simples (flecha).
                        // Igualmente los visitamos para renderizar la flecha.
                    }
                }

                // El id del componente es el valor de la propiedad `name`, si existe;
                // si no, usamos el nombre del símbolo como fallback.
                let cid = c.properties.get("name")
                    .cloned()
                    .unwrap_or_else(|| sym_name.to_owned());

                let child_gt = gt.child(c.x, c.y, c.rotation, c.flip != 0);

                let mut comp_props = c.properties.clone();
                comp_props.insert("symname".into(), sym_name.into());
                comp_props.entry("spice_get_voltage".into()).or_default();
                comp_props.entry("spice_get_current".into()).or_default();

                match self.resolve_symbol(&sym_file) {
                    Some(objects) => {
                        let objects = Arc::clone(&objects);
                        // Como Xschem: un atributo que la instancia no define
                        // toma el valor del `template` del símbolo (`@mult`,
                        // `@MF`…), así los textos calculados no quedan vacíos.
                        for (k, v) in template_defaults(&objects) {
                            comp_props.entry(k).or_insert(v);
                        }
                        self.visit_objects(&objects, child_gt, &comp_props, Some(&cid));
                    }
                    None => {
                        let (mx, my) = gt.apply(c.x, c.y);
                        self.elements.push(DrawElement::MissingSymbol {
                            name: sym_file.clone(),
                            x: mx,
                            y: my,
                            component_id: Some(cid),
                        });
                        if !self.missing.contains(&sym_file) {
                            self.missing.push(sym_file);
                        }
                    }
                }
            }

            Object::EmbeddedSymbol(objects) => {
                self.visit_objects(objects, gt, parent_props, component_id);
            }

            Object::GlobalProperties(_)
            | Object::Version(_)
            | Object::Spice(_)
            | Object::Verilog(_)
            | Object::Spectre(_)
            | Object::Vhdl(_)
            | Object::Tedax(_) => {}
        }
    }

    pub(crate) fn resolve_symbol(&mut self, sym_file: &str) -> Option<Arc<Vec<Object>>> {
        if let Some(cached) = self.sym_cache.get(sym_file) {
            return Some(Arc::clone(cached));
        }

        if let Some(content) = self.opts.symbol_lookup.as_ref().and_then(|lookup| lookup(sym_file)) {
            if let Ok(sch) = parser::parse(&content) {
                let arc = Arc::new(sch.objects);
                self.sym_cache.insert(sym_file.to_string(), Arc::clone(&arc));
                return Some(arc);
            }
        }

        let filename = sym_file.split('/').last().unwrap_or(sym_file);
        let no_slash = !sym_file.contains('/');

        for base in &self.opts.symbol_paths {
            let mut candidates = vec![base.join(sym_file), base.join(filename)];
            if no_slash {
                candidates.push(base.join("devices").join(filename));
            }
            for candidate in candidates {
                if let Ok(content) = std::fs::read_to_string(&candidate) {
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

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Valores por defecto del símbolo (`template=` de su bloque `K`).
pub(crate) fn template_defaults(objects: &[Object]) -> Properties {
    objects
        .iter()
        .find_map(|o| match o {
            Object::GlobalProperties(p) => p.get("template"),
            _ => None,
        })
        .map(|t| parser::parse_property_string(t))
        .unwrap_or_default()
}

fn substitute_attrs(text: &str, props: &Properties) -> String {
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
            result.push_str(props.get(&attr).map(|s| s.as_str()).unwrap_or(""));
        } else {
            result.push(c);
        }
    }
    result
}

// ─── Arc endpoint helper (usado por el renderer SVG) ─────────────────────────

pub fn arc_endpoints(cx: f64, cy: f64, r: f64, start_deg: f64, sweep_deg: f64)
    -> ((f64, f64), (f64, f64), bool)
{
    let start = -start_deg;
    let sweep = -sweep_deg;
    let x1 = cx + r * (start * PI / 180.0).cos();
    let y1 = cy + r * (start * PI / 180.0).sin();
    let x2 = cx + r * ((start + sweep) * PI / 180.0).cos();
    let y2 = cy + r * ((start + sweep) * PI / 180.0).sin();
    let large_arc = sweep.abs() > 180.0;
    ((x1, y1), (x2, y2), large_arc)
}

#[cfg(test)]
mod orientation_tests {
    use super::*;
    use crate::models::DrawElement;

    /// Símbolo tipo capacitor: pin `c0` arriba y un arco bajo la placa, con
    /// un texto que usa un atributo que solo está en el `template`.
    const SYM: &str = "v {xschem version=3.1.0 file_version=1.2}
K {type=capacitor template=\"name=C1 MF=2\"}
L 4 -10 -5 10 -5 {}
B 5 -2.5 -32.5 2.5 -27.5 {name=c0 dir=inout}
A 4 0 23.75 21.25 61.92751306414704 56.14497387170592 {}
T {MF=@MF} 17.5 18.75 0 0 0.2 0.2 {}
";

    fn scene(rot: i32, flip: i32) -> ResolvedScene {
        let dir = std::env::temp_dir().join(format!("xv-orient-{}-{rot}-{flip}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("cap.sym"), SYM).unwrap();
        let sch = format!("v {{xschem version=3.1.0 file_version=1.2}}\nC {{cap.sym}} 0 0 {rot} {flip} {{name=C1}}\n");
        let opts = RenderOptions::dark().with_sym_path(&dir);
        let s = SceneBuilder::new(&opts).build(&parser::parse(&sch).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
        s
    }

    fn arc_endpoints_of(s: &ResolvedScene) -> ((f64, f64), (f64, f64)) {
        s.elements
            .iter()
            .find_map(|e| match e {
                DrawElement::Arc { cx, cy, r, start_angle, sweep_angle, .. } => {
                    let (a, b, _) = arc_endpoints(*cx, *cy, *r, *start_angle, *sweep_angle);
                    Some((a, b))
                }
                _ => None,
            })
            .unwrap()
    }

    fn near(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6
    }

    #[test]
    fn rotated_arc_stays_next_to_the_plate() {
        // Como Xschem: rotar 90° lleva (10, 5) y (−10, 5) a (−5, 10) y (−5, −10).
        let (a, b) = arc_endpoints_of(&scene(1, 0));
        assert!(near(a, (-5.0, 10.0)) && near(b, (-5.0, -10.0)) || near(a, (-5.0, -10.0)) && near(b, (-5.0, 10.0)), "{a:?} {b:?}");
        let (a, b) = arc_endpoints_of(&scene(3, 1));
        assert!((a.0 - 5.0).abs() < 1e-6 && (b.0 - 5.0).abs() < 1e-6, "{a:?} {b:?}");
    }

    #[test]
    fn flip_is_applied_before_rotation() {
        // Pin c0 (0, −30) en r1 con espejo: Xschem lo deja a la derecha (30, 0).
        let s = scene(1, 1);
        let pin = s
            .elements
            .iter()
            .find_map(|e| match e {
                DrawElement::Rect { x, y, w, h, layer: 5, .. } => Some((x + w / 2.0, y + h / 2.0)),
                _ => None,
            })
            .unwrap();
        assert!(near(pin, (30.0, 0.0)), "{pin:?}");
    }

    #[test]
    fn missing_attributes_come_from_the_template() {
        let s = scene(0, 0);
        assert!(s.elements.iter().any(|e| matches!(e, DrawElement::Text { content, .. } if content == "MF=2")));
    }
}

#[cfg(test)]
mod lookup_tests {
    use super::*;
    use crate::renderer::RenderOptions;

    #[test]
    fn symbol_lookup_se_consulta_antes_que_el_disco() {
        let sym = "v {xschem version=3.4.5 file_version=1.2}\nL 4 0 0 10 0 {}\n";
        let lookup: crate::renderer::SymbolLookup = Arc::new(move |s: &str| (s == "mio.sym").then(|| sym.to_string()));
        let opts = RenderOptions::dark().with_symbol_lookup(lookup);
        let sch = parser::parse("v {xschem version=3.4.5 file_version=1.2}\nC {mio.sym} 0 0 0 0 {name=x1}\n").unwrap();
        let rs = SceneBuilder::new(&opts).build(&sch);
        assert!(rs.missing_symbols.is_empty(), "{:?}", rs.missing_symbols);
        assert!(rs.elements_of("x1").next().is_some());
    }
}
