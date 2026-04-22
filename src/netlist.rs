use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::models::{Object, ResolvedScene, Schematic};

// ─── Public model ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct Pin {
    /// Component instance name, e.g. "R1", "M3"
    pub instance: String,
    /// Pin name as declared in the symbol, e.g. "A", "DRAIN"
    pub pin: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Net {
    pub name: String,
    /// All pins connected to this net
    pub pins: Vec<Pin>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    /// Value of the `name` property, e.g. "R1"
    pub name: String,
    /// Symbol path, e.g. "sky130_fd_pr/nfet_01v8.sym"
    pub symbol: String,
    /// All other properties (value, W, L, model, …)
    pub params: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Netlist {
    pub instances: BTreeMap<String, Instance>,
    /// Nets keyed by net name
    pub nets: BTreeMap<String, Net>,
}

impl Netlist {
    /// Net names that appear in this netlist
    pub fn net_names(&self) -> BTreeSet<&str> {
        self.nets.keys().map(|s| s.as_str()).collect()
    }
}

// ─── Extraction ───────────────────────────────────────────────────────────────

/// Extract a flat netlist from a parsed schematic.
///
/// Pin-to-net connectivity is built from wire labels and text labels (`lab=` property).
/// Components without a `name` property are skipped (e.g. `code_shown`, `corner`).
pub fn extract_netlist(schematic: &Schematic) -> Netlist {
    let mut netlist = Netlist::default();

    // Pass 1: collect instances from Component objects
    for obj in &schematic.objects {
        if let Object::Component(c) = obj {
            let Some(name) = c.properties.get("name") else { continue };
            if name.is_empty() { continue; }

            let symbol = c.symbol_reference
                .trim_end_matches(".sym")
                .to_string();

            let params: BTreeMap<String, String> = c.properties
                .iter()
                .filter(|(k, _)| k.as_str() != "name")
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();

            netlist.instances.insert(name.clone(), Instance {
                name: name.clone(),
                symbol,
                params,
            });
        }
    }

    // Pass 2: collect net names from wire labels and text labels
    collect_nets(schematic, &mut netlist);

    netlist
}

// ─── Topology / pin-to-net connectivity ──────────────────────────────────────

/// Tolerancia en unidades xschem para considerar que un endpoint de wire toca un pin.
/// Los grids de xschem son múltiplos de 5; 2.5 cubre errores de redondeo sin falsos positivos.
const SNAP_TOLERANCE: f64 = 2.5;

/// Rellena `Net.pins` en el netlist usando matching geométrico entre endpoints de wires
/// (del `ResolvedScene`) y posiciones de pines de instancia.
///
/// El algoritmo:
/// 1. Construye un mapa de punto → net_name a partir de wire labels del schematic.
/// 2. Para cada pin de cada instancia, busca el endpoint de wire más cercano dentro
///    de `SNAP_TOLERANCE`. Si el wire lleva `lab=`, ese es el net name; si no, propaga
///    el net name del primer wire conectado al cluster.
/// 3. Actualiza `Net.pins`.
///
/// Esta función es O(P × W) donde P = número de pines, W = número de endpoints.
/// En esquemas reales (<50 instancias, <200 wires) esto es <1 ms.
pub fn fill_connectivity(netlist: &mut Netlist, scene: &ResolvedScene, schematic: &Schematic) {
    // Mapa: (x, y) redondeado → net_name (de wires con lab=)
    let labeled: HashMap<(i64, i64), String> = schematic.objects
        .iter()
        .filter_map(|o| {
            if let Object::Wire(w) = o {
                w.properties.get("lab").filter(|s| !s.is_empty()).map(|lab| {
                    // Ambos endpoints del wire etiquetado pertenecen a ese net
                    vec![
                        (snap(w.x1, w.y1), lab.clone()),
                        (snap(w.x2, w.y2), lab.clone()),
                    ]
                })
            } else {
                None
            }
        })
        .flatten()
        .collect();

    // También capturamos text labels (componentes ipin/opin con lab=)
    let mut label_map: HashMap<(i64, i64), String> = labeled;
    for obj in &schematic.objects {
        if let Object::Component(c) = obj {
            let sym = c.symbol_reference.split('/').last().unwrap_or(&c.symbol_reference);
            let sym = sym.trim_end_matches(".sym");
            if matches!(sym, "ipin" | "opin" | "iopin" | "label") {
                if let Some(lab) = c.properties.get("lab") {
                    if !lab.is_empty() {
                        label_map.insert(snap(c.x, c.y), lab.clone());
                    }
                }
            }
        }
    }

    // Para cada instancia con pines resueltos, buscar el net más cercano
    for (instance_name, pins) in &scene.pin_positions {
        for (pin_name, px, py) in pins {
            if let Some(net_name) = nearest_net(&label_map, *px, *py) {
                netlist.nets
                    .entry(net_name.clone())
                    .or_insert_with(|| Net { name: net_name.clone(), pins: Vec::new() })
                    .pins
                    .push(Pin { instance: instance_name.clone(), pin: pin_name.clone() });
            }
        }
    }
}

fn snap(x: f64, y: f64) -> (i64, i64) {
    ((x * 10.0).round() as i64, (y * 10.0).round() as i64)
}

fn nearest_net(label_map: &HashMap<(i64, i64), String>, x: f64, y: f64) -> Option<String> {
    let tol = (SNAP_TOLERANCE * 10.0) as i64;
    let (sx, sy) = snap(x, y);
    label_map
        .iter()
        .filter(|((lx, ly), _)| (lx - sx).abs() <= tol && (ly - sy).abs() <= tol)
        .min_by_key(|((lx, ly), _)| {
            let dx = lx - sx;
            let dy = ly - sy;
            dx * dx + dy * dy
        })
        .map(|(_, name)| name.clone())
}

fn collect_nets(schematic: &Schematic, netlist: &mut Netlist) {
    for obj in &schematic.objects {
        match obj {
            Object::Wire(w) => {
                if let Some(lab) = w.properties.get("lab") {
                    if !lab.is_empty() {
                        netlist.nets.entry(lab.clone()).or_insert_with(|| Net {
                            name: lab.clone(),
                            pins: Vec::new(),
                        });
                    }
                }
            }
            Object::Text(t) => {
                // Net labels: text objects with lab= property
                if let Some(lab) = t.properties.get("lab") {
                    if !lab.is_empty() {
                        netlist.nets.entry(lab.clone()).or_insert_with(|| Net {
                            name: lab.clone(),
                            pins: Vec::new(),
                        });
                    }
                }
            }
            Object::Component(c) => {
                // ipin/opin/iopin/label symbols define net names via lab= property
                let sym = c.symbol_reference.split('/').last().unwrap_or(&c.symbol_reference);
                let sym = sym.trim_end_matches(".sym");
                if matches!(sym, "ipin" | "opin" | "iopin" | "label") {
                    if let Some(lab) = c.properties.get("lab") {
                        if !lab.is_empty() {
                            netlist.nets.entry(lab.clone()).or_insert_with(|| Net {
                                name: lab.clone(),
                                pins: Vec::new(),
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    fn netlist(input: &str) -> Netlist {
        extract_netlist(&parse(input).unwrap())
    }

    #[test]
    fn empty_schematic() {
        let nl = netlist("");
        assert!(nl.instances.is_empty());
        assert!(nl.nets.is_empty());
    }

    #[test]
    fn single_component() {
        let nl = netlist("C {devices/res.sym} 0 0 0 0 {name=R1 value=1k}");
        assert!(nl.instances.contains_key("R1"));
        let inst = &nl.instances["R1"];
        assert_eq!(inst.symbol, "devices/res");
        assert_eq!(inst.params.get("value").map(|s| s.as_str()), Some("1k"));
        assert!(!inst.params.contains_key("name"));
    }

    #[test]
    fn component_without_name_skipped() {
        let nl = netlist("C {corner.sym} 0 0 0 0 {only_toplevel=true}");
        assert!(nl.instances.is_empty());
    }

    #[test]
    fn wire_with_label_creates_net() {
        let nl = netlist("N 0 0 100 0 {lab=Vdd}");
        assert!(nl.nets.contains_key("Vdd"));
    }

    #[test]
    fn ipin_creates_net() {
        let nl = netlist("C {ipin.sym} 0 0 0 0 {name=p1 lab=Vdd}");
        assert!(nl.nets.contains_key("Vdd"));
    }

    #[test]
    fn multiple_components_and_nets() {
        let input = "\
C {devices/res.sym} 0 0 0 0 {name=R1 value=1k}
C {devices/res.sym} 0 100 0 0 {name=R2 value=2k}
N 0 0 100 0 {lab=Vdd}
N 0 100 100 100 {lab=GND}";
        let nl = netlist(input);
        assert_eq!(nl.instances.len(), 2);
        assert_eq!(nl.nets.len(), 2);
        assert!(nl.nets.contains_key("Vdd"));
        assert!(nl.nets.contains_key("GND"));
    }

    #[test]
    fn net_names_returns_all_nets() {
        let input = "N 0 0 10 0 {lab=A}\nN 0 10 10 10 {lab=B}";
        let nl = netlist(input);
        let names = nl.net_names();
        assert!(names.contains("A"));
        assert!(names.contains("B"));
    }
}
