//! Netlist SPICE de un esquemático, como la que escribe Xschem, sin Xschem.
//!
//! - **Conectividad:** dos wires se unen si comparten un extremo o si el
//!   extremo de uno cae sobre el otro; un pin se une al wire que lo toca (en
//!   cualquier punto) y a los pines que están en el mismo lugar. Las nets con
//!   el mismo nombre son la misma net.
//! - **Nombres:** los de las etiquetas (`lab_pin`, `ipin`, `opin`, `iopin`:
//!   tipos `label`, `ipin`, `opin`, `iopin`) y el `lab` de los wires; las
//!   demás, `net1`, `net2`…
//! - **Cada instancia:** el `format` de su símbolo (`lvs_format` en modo LVS)
//!   con `@name`, `@pinlist` (en el orden de los pines del símbolo), `@@PIN`,
//!   `@symname`, `@spiceprefix` y sus atributos (los que no pone la
//!   instancia, del `template` del símbolo); `tcleval(…)` se evalúa.
//! - **Sub-circuitos:** un símbolo de tipo `subcircuit` se escribe como
//!   `.subckt` con su esquemático, una vez por símbolo.
//!
//! No se escriben las etiquetas, los textos de comandos (`netlist_commands`)
//! ni lo que lleva `spice_ignore` (o `lvs_ignore`, en modo LVS).

use std::collections::{BTreeMap, HashMap};

use crate::models::{Object, Properties};
use crate::parser;
use crate::renderer::RenderOptions;
use crate::scene::{template_defaults, SceneBuilder, Transform};

/// Cómo escribir la netlist.
#[derive(Clone, Copy, Debug, Default)]
pub struct SpiceOptions {
    /// Modo LVS: `lvs_format` si el símbolo lo tiene y `lvs_ignore`.
    pub lvs: bool,
    /// El esquemático de arriba también como `.subckt` (con sus pines).
    pub top_subckt: bool,
}

/// El texto de un sub-esquemático: `(referencia, esquemático que la usa)` →
/// `(ruta, contenido)`. La referencia es la del atributo `schematic=` o la
/// del símbolo con `.sch` (`amp.sym` → `amp.sch`).
pub type SchematicLookup<'a> = &'a dyn Fn(&str, &str) -> Option<(String, String)>;

/// La netlist y lo que no se pudo resolver.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Spice {
    pub text: String,
    pub warnings: Vec<String>,
}

/// La netlist de `text` (el esquemático `path`, de nombre `name`).
pub fn netlist(text: &str, path: &str, name: &str, opts: &RenderOptions, spice: SpiceOptions, lookup: SchematicLookup<'_>) -> Result<Spice, String> {
    let mut n = Netlister { builder: SceneBuilder::new(opts), spice, lookup, defined: BTreeMap::new(), warnings: Vec::new() };
    let top = n.cell(text, path, true)?;
    let mut out = String::new();
    out.push_str(&format!("** {name}\n"));
    if spice.top_subckt {
        // Los pines, los del símbolo del esquemático (`amp.sch` → `amp.sym`)
        // si tiene uno, en su orden; si no, sus `ipin`/`opin`/`iopin`.
        let stem = path.rsplit('/').next().unwrap_or(path).trim_end_matches(".sch");
        let ports = match n.builder.resolve_symbol(&format!("{stem}.sym")).map(|o| symbol_of(&o, spice.lvs).ports()) {
            Some(ports) if !ports.is_empty() => ports,
            _ => top.ports,
        };
        out.push_str(&format!(".subckt {name} {}\n", ports.join(" ")));
        out.push_str(&top.body);
        out.push_str(".ends\n");
    } else {
        out.push_str(&top.body);
    }
    for def in n.defined.values().flatten() {
        out.push('\n');
        out.push_str(def);
    }
    out.push_str(".end\n");
    Ok(Spice { text: out, warnings: n.warnings })
}

/// Un pin de un símbolo: su nombre y su centro (coordenadas del símbolo).
#[derive(Clone, Debug)]
struct SymPin {
    name: String,
    x: f64,
    y: f64,
}

/// Lo que importa de un símbolo para la netlist.
struct Symbol {
    kind: String,
    format: Option<String>,
    template: Properties,
    props: Properties,
    pins: Vec<SymPin>,
}

impl Symbol {
    /// Los puertos de su `.subckt`: sus pines y los de `extra` (p. ej.
    /// `extra="VCCPIN VSSPIN"`, la alimentación que no se dibuja).
    fn ports(&self) -> Vec<String> {
        let extra = self.props.get("extra").map(|e| e.split_whitespace().map(str::to_string).collect::<Vec<_>>()).unwrap_or_default();
        self.pins.iter().map(|p| p.name.clone()).chain(extra).collect()
    }
}

fn symbol_of(objects: &[Object], lvs: bool) -> Symbol {
    let props: Properties = objects
        .iter()
        .find_map(|o| match o {
            Object::GlobalProperties(p) if p.contains_key("type") || p.contains_key("format") => Some(p.clone()),
            _ => None,
        })
        .unwrap_or_default();
    let format = if lvs { props.get("lvs_format").or(props.get("format")) } else { props.get("format") }.cloned();
    let mut pins: Vec<(Option<i64>, usize, SymPin)> = objects
        .iter()
        .filter_map(|o| match o {
            Object::Rectangle(r) if r.layer == 5 => Some(r),
            _ => None,
        })
        .filter_map(|r| {
            let name = r.properties.get("name")?.clone();
            let order = r.properties.get("sim_pinnumber").and_then(|v| v.trim().parse().ok());
            Some((order, 0, SymPin { name, x: (r.x1 + r.x2) / 2.0, y: (r.y1 + r.y2) / 2.0 }))
        })
        .enumerate()
        .map(|(i, (o, _, p))| (o, i, p))
        .collect();
    // `sim_pinnumber` manda si está; si no, el orden del archivo.
    pins.sort_by_key(|(o, i, _)| (o.unwrap_or(i64::MAX), *i));
    Symbol {
        kind: props.get("type").cloned().unwrap_or_default(),
        format,
        template: template_defaults(objects),
        props,
        pins: pins.into_iter().map(|(_, _, p)| p).collect(),
    }
}

/// Un esquemático ya pasado a netlist.
struct Cell {
    /// Las nets de sus pines (`ipin`/`opin`/`iopin`), en orden.
    ports: Vec<String>,
    /// Una línea (o varias, con `+`) por instancia.
    body: String,
}

struct Netlister<'a> {
    builder: SceneBuilder<'a>,
    spice: SpiceOptions,
    lookup: SchematicLookup<'a>,
    /// `.subckt` de cada símbolo ya escrito (`None` mientras se escribe:
    /// corta los ciclos).
    defined: BTreeMap<String, Option<String>>,
    warnings: Vec<String>,
}

/// Una instancia a escribir.
struct Inst {
    name: String,
    symname: String,
    symbol: Symbol,
    attrs: Properties,
    /// El nodo de cada pin del símbolo (índice en la unión de nodos).
    pin_nodes: Vec<usize>,
    /// Referencia del sub-esquemático, si es un `subcircuit`.
    schematic: Option<String>,
}

impl<'a> Netlister<'a> {
    /// `top`: el esquemático de arriba (los bloques de código con
    /// `only_toplevel=true` solo se escriben ahí).
    fn cell(&mut self, text: &str, path: &str, top: bool) -> Result<Cell, String> {
        let sch = parser::parse(text).map_err(|e| format!("{path}: {e}"))?;

        // Nodos: cada wire y cada pin de instancia.
        let wires: Vec<_> = sch.objects.iter().filter_map(|o| if let Object::Wire(w) = o { Some(w) } else { None }).collect();
        let mut uf = UnionFind::new(wires.len());
        let mut pin_points: Vec<(usize, (f64, f64))> = Vec::new();
        let mut insts: Vec<Inst> = Vec::new();
        let mut labels: Vec<(usize, String, &'static str)> = Vec::new();

        for c in sch.components() {
            let sym_file = if c.symbol_reference.ends_with(".sym") { c.symbol_reference.clone() } else { format!("{}.sym", c.symbol_reference) };
            let symname = sym_file.rsplit('/').next().unwrap_or(&sym_file).trim_end_matches(".sym").to_string();
            let Some(objects) = self.builder.resolve_symbol(&sym_file) else {
                self.warnings.push(format!("{path}: no se encontró el símbolo {sym_file}"));
                continue;
            };
            let symbol = symbol_of(&objects, self.spice.lvs);
            let mut attrs = symbol.template.clone();
            for (k, v) in &c.properties {
                attrs.insert(k.clone(), v.clone());
            }
            let xf = Transform::identity().child(c.x, c.y, c.rotation, c.flip != 0);
            let pin_nodes: Vec<usize> = symbol
                .pins
                .iter()
                .map(|p| {
                    let node = uf.push();
                    pin_points.push((node, xf.apply(p.x, p.y)));
                    node
                })
                .collect();
            match symbol.kind.as_str() {
                k @ ("label" | "ipin" | "opin" | "iopin") => {
                    if let (Some(lab), Some(&node)) = (attrs.get("lab"), pin_nodes.first()) {
                        let role = match k {
                            "ipin" => "ipin",
                            "opin" => "opin",
                            "iopin" => "iopin",
                            _ => "label",
                        };
                        labels.push((node, lab.clone(), role));
                    }
                    continue;
                }
                _ => {}
            }
            let schematic = (symbol.kind == "subcircuit")
                .then(|| attrs.get("schematic").or(symbol.props.get("schematic")).cloned().unwrap_or_else(|| sym_file.replace(".sym", ".sch")));
            insts.push(Inst { name: attrs.get("name").cloned().unwrap_or_default(), symname, symbol, attrs, pin_nodes, schematic });
        }

        // Wires: extremos compartidos o sobre otro wire.
        let segs: Vec<((f64, f64), (f64, f64))> = wires.iter().map(|w| ((w.x1, w.y1), (w.x2, w.y2))).collect();
        for (i, a) in segs.iter().enumerate() {
            for (j, b) in segs.iter().enumerate().skip(i + 1) {
                if on_segment(a.0, *b) || on_segment(a.1, *b) || on_segment(b.0, *a) || on_segment(b.1, *a) {
                    uf.union(i, j);
                }
            }
        }
        // Pines: sobre un wire, o en el mismo punto que otro pin.
        let mut by_point: HashMap<(i64, i64), usize> = HashMap::new();
        for &(node, p) in &pin_points {
            for (i, s) in segs.iter().enumerate() {
                if on_segment(p, *s) {
                    uf.union(node, i);
                }
            }
            match by_point.entry(key(p)) {
                std::collections::hash_map::Entry::Occupied(e) => uf.union(node, *e.get()),
                std::collections::hash_map::Entry::Vacant(e) => {
                    e.insert(node);
                }
            }
        }

        // Nombres: etiquetas y `lab` de los wires; el mismo nombre, la misma net.
        let mut named: BTreeMap<String, usize> = BTreeMap::new();
        let mut name_of = |uf: &mut UnionFind, node: usize, name: &str| {
            let name = name.trim();
            if name.is_empty() {
                return;
            }
            match named.get(name) {
                Some(&other) => uf.union(node, other),
                None => {
                    named.insert(name.to_string(), node);
                }
            }
        };
        for (node, lab, _) in &labels {
            name_of(&mut uf, *node, lab);
        }
        // `#net3`: el nombre que Xschem le puso a una net sin nombre la
        // última vez. Puede estar viejo, así que no une nets: solo se usa
        // (sin `#`) si nadie más lo tiene.
        let mut auto: Vec<(usize, String)> = Vec::new();
        for (i, w) in wires.iter().enumerate() {
            match w.properties.get("lab").map(|l| l.trim()) {
                Some(lab) if lab.starts_with('#') => auto.push((i, lab.trim_start_matches('#').to_string())),
                Some(lab) => name_of(&mut uf, i, lab),
                None => {}
            }
        }
        let mut net_name: HashMap<usize, String> = HashMap::new();
        for (name, node) in &named {
            net_name.entry(uf.find(*node)).or_insert_with(|| name.clone());
        }
        let mut taken: std::collections::HashSet<String> = net_name.values().cloned().collect();
        for (node, lab) in auto {
            let root = uf.find(node);
            if !net_name.contains_key(&root) && !lab.is_empty() && taken.insert(lab.clone()) {
                net_name.insert(root, lab);
            }
        }
        let mut next = 1;
        let mut name = |uf: &mut UnionFind, node: usize| -> String {
            let root = uf.find(node);
            net_name
                .entry(root)
                .or_insert_with(|| loop {
                    let n = format!("net{next}");
                    next += 1;
                    if taken.insert(n.clone()) {
                        break n;
                    }
                })
                .clone()
        };

        let ports: Vec<String> = labels.iter().filter(|(_, _, r)| *r != "label").map(|(n, _, _)| name(&mut uf, *n)).collect();

        let mut body = String::new();
        let mut commands: Vec<String> = Vec::new();
        for inst in &insts {
            let skip = |key: &str| {
                [inst.attrs.get(key), inst.symbol.props.get(key)].into_iter().flatten().any(|v| matches!(v.trim(), "true" | "open" | "short"))
            };
            if skip("spice_ignore") || (self.spice.lvs && skip("lvs_ignore")) || inst.symbol.kind == "launcher" {
                continue;
            }
            // Bloques de código: Xschem los escribe (también en modo LVS)
            // después de los dispositivos.
            if inst.symbol.kind == "netlist_commands" {
                let only_top = inst.attrs.get("only_toplevel").is_some_and(|v| v.trim() == "true");
                if top || !only_top {
                    let format = inst.symbol.format.clone().unwrap_or_else(|| "@value".to_string());
                    commands.push(expand(&format, &inst.name, &inst.symname, &inst.attrs, &[]));
                }
                continue;
            }
            let Some(format) = inst.symbol.format.clone() else { continue };
            let pins: Vec<(String, String)> =
                inst.symbol.pins.iter().zip(&inst.pin_nodes).map(|(p, &node)| (p.name.clone(), name(&mut uf, node))).collect();
            let line = expand(&format, &inst.name, &inst.symname, &inst.attrs, &pins);
            let line = line.trim();
            if !line.is_empty() {
                body.push_str(line);
                body.push('\n');
            }
            if let Some(reference) = &inst.schematic {
                self.define(&inst.symname, reference, path, &inst.symbol.ports());
            }
        }
        for c in commands {
            let c = c.trim();
            if !c.is_empty() {
                body.push_str(c);
                body.push('\n');
            }
        }
        Ok(Cell { ports, body })
    }

    /// Escribe el `.subckt` de un símbolo, una vez, con su esquemático.
    fn define(&mut self, symname: &str, reference: &str, from: &str, ports: &[String]) {
        if self.defined.contains_key(symname) {
            return;
        }
        self.defined.insert(symname.to_string(), None);
        let Some((path, text)) = (self.lookup)(reference, from) else {
            self.warnings.push(format!("{from}: no se encontró el esquemático de {symname} ({reference})"));
            return;
        };
        match self.cell(&text, &path, false) {
            Ok(cell) => {
                let def = format!(".subckt {symname} {}\n{}.ends\n", ports.join(" "), cell.body);
                self.defined.insert(symname.to_string(), Some(def));
            }
            Err(e) => self.warnings.push(e),
        }
    }
}

/// El `format` de un símbolo con los datos de una instancia.
fn expand(format: &str, name: &str, symname: &str, attrs: &Properties, pins: &[(String, String)]) -> String {
    let chars: Vec<char> = format.chars().collect();
    let mut out = String::with_capacity(format.len() + 32);
    let mut i = 0;
    let ident = |start: usize| -> usize {
        let mut j = start;
        while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_' || chars[j] == '#' || chars[j] == ':') {
            j += 1;
        }
        j
    };
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            out.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if c != '@' {
            out.push(c);
            i += 1;
            continue;
        }
        // `@@PIN`: la net de un pin.
        if chars.get(i + 1) == Some(&'@') {
            let end = ident(i + 2);
            let pin: String = chars[i + 2..end].iter().collect();
            if let Some((_, net)) = pins.iter().find(|(p, _)| *p == pin) {
                out.push_str(net);
            }
            i = end;
            continue;
        }
        let end = ident(i + 1);
        let key: String = chars[i + 1..end].iter().collect();
        match key.as_str() {
            "" => out.push('@'),
            "name" => out.push_str(name),
            "symname" => out.push_str(symname),
            "pinlist" => out.push_str(&pins.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>().join(" ")),
            "path" => {}
            // `savecurrent=true`: la corriente se guarda (`.save i(v1)`).
            "savecurrent" => {
                if attrs.get("savecurrent").is_some_and(|v| v.trim() == "true") {
                    out.push_str(&format!("\n.save i({})", name.to_lowercase()));
                }
            }
            _ => {
                let value = attrs.get(&key).map(|v| v.trim()).unwrap_or("");
                // `clave=@attr` sin valor no se escribe; `m=1` tampoco (es
                // el valor por omisión), como en Xschem.
                let token = out.rfind(char::is_whitespace).map_or(0, |p| p + 1);
                let assign = out[token..].strip_suffix('=').filter(|k| !k.is_empty() && !k.contains('@'));
                match assign {
                    Some(k) if value.is_empty() || (k == "m" && value == "1") => {
                        out.truncate(token);
                        // Lo que siga pegado al valor (hasta el espacio) tampoco.
                        let mut j = end;
                        while j < chars.len() && !chars[j].is_whitespace() {
                            j += 1;
                        }
                        i = j;
                        continue;
                    }
                    _ => out.push_str(value),
                }
            }
        }
        i = end;
    }
    // Espacios de más donde se quitaron tokens.
    let out: String = out.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).collect::<Vec<_>>().join("\n");
    if out.contains("tcleval(") {
        crate::tcleval::eval_text(&out)
    } else {
        out
    }
}

/// Punto para comparar (las coordenadas de Xschem van en múltiplos chicos).
fn key((x, y): (f64, f64)) -> (i64, i64) {
    ((x * 1000.0).round() as i64, (y * 1000.0).round() as i64)
}

/// `p` está sobre el segmento `s` (extremos incluidos).
fn on_segment(p: (f64, f64), s: ((f64, f64), (f64, f64))) -> bool {
    const EPS: f64 = 1e-6;
    let ((x1, y1), (x2, y2)) = s;
    let cross = (x2 - x1) * (p.1 - y1) - (y2 - y1) * (p.0 - x1);
    let len = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt().max(1.0);
    cross.abs() <= EPS * len
        && p.0 >= x1.min(x2) - EPS
        && p.0 <= x1.max(x2) + EPS
        && p.1 >= y1.min(y2) - EPS
        && p.1 <= y1.max(y2) + EPS
}

struct UnionFind(Vec<usize>);

impl UnionFind {
    fn new(n: usize) -> Self {
        Self((0..n).collect())
    }

    fn push(&mut self) -> usize {
        self.0.push(self.0.len());
        self.0.len() - 1
    }

    fn find(&mut self, x: usize) -> usize {
        let mut r = x;
        while self.0[r] != r {
            r = self.0[r];
        }
        let mut y = x;
        while self.0[y] != r {
            let next = self.0[y];
            self.0[y] = r;
            y = next;
        }
        r
    }

    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.0[rb] = ra;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    const RES: &str = "v {xschem version=3.4.5 file_version=1.2}\nK {type=resistor\nformat=\"@name @pinlist @value m=@m\"\ntemplate=\"name=R1 value=1k m=1\"}\nB 5 -2.5 -32.5 2.5 -27.5 {name=P dir=inout}\nB 5 -2.5 27.5 2.5 32.5 {name=M dir=inout}\n";
    const PIN: &str = "v {xschem version=3.4.5 file_version=1.2}\nK {type=ipin\nformat=\"*.ipin @lab\"\ntemplate=\"name=p1 lab=xxx\"}\nB 5 -2.5 -2.5 2.5 2.5 {name=p dir=in}\n";
    const AMP_SYM: &str = "v {xschem version=3.4.5 file_version=1.2}\nK {type=subcircuit\nformat=\"@name @pinlist @symname\"\ntemplate=\"name=x1\"}\nB 5 -2.5 -2.5 2.5 2.5 {name=in dir=in}\nB 5 97.5 -2.5 102.5 2.5 {name=out dir=out}\n";

    fn opts() -> RenderOptions {
        let syms: std::collections::HashMap<&'static str, &'static str> = [("res.sym", RES), ("ipin.sym", PIN), ("amp.sym", AMP_SYM)].into_iter().collect();
        RenderOptions::dark().with_symbol_lookup(Arc::new(move |s: &str| syms.get(s).map(|t| t.to_string())))
    }

    #[test]
    fn dos_resistores_en_serie_con_una_net_sin_nombre() {
        // R1 de (0,-30) a (0,30); R2 de (100,-30) a (100,30); un wire une el
        // pin M de R1 con el P de R2; `in` etiqueta el P de R1.
        let sch = "v {xschem version=3.4.5 file_version=1.2}\n\
C {res.sym} 0 0 0 0 {name=R1 value=1k}\n\
C {res.sym} 100 0 0 0 {name=R2 value=2k}\n\
N 0 30 100 -30 {}\n\
C {ipin.sym} 0 -30 0 0 {name=p1 lab=in}\n";
        let s = netlist(sch, "t.sch", "t", &opts(), SpiceOptions { lvs: true, top_subckt: true }, &|_, _| None).unwrap();
        assert!(s.text.contains(".subckt t in\n"), "{}", s.text);
        assert!(s.text.contains("R1 in net1 1k\n"), "{}", s.text);
        assert!(s.text.contains("R2 net1 net2 2k\n"), "{}", s.text);
        assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    }

    #[test]
    fn los_nombres_de_xschem_con_numeral_no_unen_nets() {
        // Dos wires sueltos con el mismo `#net1` viejo: siguen separados; el
        // primero se queda con `net1` y el otro recibe uno nuevo.
        let sch = "v {xschem version=3.4.5 file_version=1.2}\n\
C {res.sym} 0 0 0 0 {name=R1 value=1k}\n\
N 0 -30 0 -60 {lab=#net1}\n\
N 0 30 0 60 {lab=#net1}\n";
        let s = netlist(sch, "t.sch", "t", &opts(), SpiceOptions::default(), &|_, _| None).unwrap();
        assert!(s.text.contains("R1 net1 net2 1k\n"), "{}", s.text);
    }

    #[test]
    fn un_subcircuito_se_escribe_una_vez() {
        let amp = "v {xschem version=3.4.5 file_version=1.2}\n\
C {res.sym} 0 0 0 0 {name=R1 value=5k}\n\
C {ipin.sym} 0 -30 0 0 {name=p1 lab=in}\n\
C {ipin.sym} 0 30 0 0 {name=p2 lab=out}\n";
        let top = "v {xschem version=3.4.5 file_version=1.2}\n\
C {amp.sym} 0 0 0 0 {name=x1}\n\
C {amp.sym} 0 200 0 0 {name=x2}\n";
        let lookup = |r: &str, _: &str| (r == "amp.sch").then(|| ("amp.sch".to_string(), amp.to_string()));
        let s = netlist(top, "top.sch", "top", &opts(), SpiceOptions::default(), &lookup).unwrap();
        assert_eq!(s.text.matches(".subckt amp in out").count(), 1, "{}", s.text);
        assert!(s.text.contains("x1 net1 net2 amp"), "{}", s.text);
        assert!(s.text.contains("R1 in out 5k"), "{}", s.text);
    }

    #[test]
    fn expande_pines_y_atributos() {
        let attrs: Properties = [("spiceprefix", "X"), ("W", "2"), ("model", "nfet")].into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let pins = vec![("D".to_string(), "out".to_string()), ("G".to_string(), "in".to_string())];
        assert_eq!(expand("@spiceprefix@name @pinlist sky130_fd_pr__@model W=@W G=@@G", "M1", "nfet", &attrs, &pins), "XM1 out in sky130_fd_pr__nfet W=2 G=in");
    }

    #[test]
    fn sin_valor_no_se_escribe_la_clave_ni_m_1() {
        let attrs: Properties = [("W", "2"), ("m", "1"), ("value", "3"), ("savecurrent", "true")].into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        assert_eq!(expand("@name W=@W nf=@nf m=@m", "M1", "nfet", &attrs, &[]), "M1 W=2");
        assert_eq!(expand("@name @value@savecurrent", "V1", "vsource", &attrs, &[]), "V1 3\n.save i(v1)");
    }
}
