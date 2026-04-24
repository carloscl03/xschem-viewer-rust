//! Representación semántica y diff estructural de schematics xschem.
//!
//! Este módulo ofrece un modelo de dominio (no de render ni de AST) adecuado
//! para comparar dos schematics al nivel de **componentes, parámetros, pins
//! y nets** — el vocabulario con el que piensa un diseñador de circuitos.
//!
//! - `SemanticSchematic`: grafo normalizado listo para diff (componentes por
//!   nombre, wires con etiqueta, nets resueltas).
//! - `parse_semantic()`: one-shot que parsea, construye la escena para
//!   resolver conectividad geométrica, y extrae la vista semántica.
//! - `diff()`: detecta componentes añadidos/removidos/modificados, cambios
//!   de parámetros, renombrados por similitud, y cambios de posición.
//!
//! Es independiente de git y de cualquier VCS — trabaja solo sobre bytes.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    extract_netlist, fill_connectivity,
    parser,
    renderer::RenderOptions,
    scene::SceneBuilder,
};

// ─── Modelo semántico ────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SemanticComponent {
    pub name: String,
    pub symbol: String,
    pub params: BTreeMap<String, String>,
    /// Conectividad pin → net. Vacío si no se pudo resolver.
    pub pins: BTreeMap<String, String>,
    pub x: f64,
    pub y: f64,
    pub rotation: i32,
    pub mirror: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SemanticWire {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub label: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SemanticSchematic {
    pub components: BTreeMap<String, SemanticComponent>,
    pub wires: Vec<SemanticWire>,
    pub nets: BTreeSet<String>,
}

// ─── Modelo de diff ──────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    Added,
    Removed,
    Modified,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ComponentDiff {
    pub name: String,
    pub kind: ChangeKind,
    pub cosmetic: bool,
    /// true si la posición/rotación/mirror también cambió, independiente del cambio semántico.
    #[serde(default)]
    pub position_changed: bool,
    pub before: Option<BTreeMap<String, String>>,
    pub after: Option<BTreeMap<String, String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DiffReport {
    pub components: Vec<ComponentDiff>,
    pub nets_added: Vec<String>,
    pub nets_removed: Vec<String>,
    pub is_move_all: bool,
}

impl DiffReport {
    pub fn is_empty(&self) -> bool {
        self.components.iter().all(|c| c.cosmetic)
            && self.nets_added.is_empty()
            && self.nets_removed.is_empty()
    }
}

// ─── Parse → SemanticSchematic ───────────────────────────────────────────────

/// Construye la vista semántica de un .sch resolviendo conectividad pin→net
/// mediante la escena geométrica. Usa símbolos de `opts` para resolver pines.
///
/// Si el contenido no es válido devuelve un schematic vacío (consistente con
/// la forma de fallo tolerante del resto del crate).
pub fn parse_semantic(content: &str, opts: &RenderOptions) -> SemanticSchematic {
    let xv_sch = match parser::parse(content) {
        Ok(s) => s,
        Err(_) => return SemanticSchematic::default(),
    };

    let mut netlist = extract_netlist(&xv_sch);
    let scene = SceneBuilder::new(opts).build(&xv_sch);
    fill_connectivity(&mut netlist, &scene, &xv_sch);

    // Index de componentes por nombre — xschem guarda el nombre en las
    // propiedades bajo "name" o "Name".
    let comp_by_name: std::collections::HashMap<String, &crate::models::Component> = xv_sch
        .components()
        .filter_map(|c| {
            c.properties.get("name")
                .or_else(|| c.properties.get("Name"))
                .map(|n| (n.clone(), c))
        })
        .collect();

    let mut out = SemanticSchematic::default();

    for (name, inst) in &netlist.instances {
        let (x, y, rotation, mirror) = comp_by_name.get(name.as_str())
            .map(|c| (c.x, c.y, c.rotation, c.flip))
            .unwrap_or((0.0, 0.0, 0, 0));

        let pins: BTreeMap<String, String> = netlist.nets.iter()
            .flat_map(|(net_name, net)| {
                net.pins.iter()
                    .filter(|p| p.instance == *name)
                    .map(move |p| (p.pin.clone(), net_name.clone()))
            })
            .collect();

        out.components.insert(
            name.clone(),
            SemanticComponent {
                name: name.clone(),
                symbol: inst.symbol.clone(),
                params: inst.params.clone(),
                pins,
                x, y, rotation, mirror,
            },
        );
    }

    for wire in xv_sch.wires() {
        let label = wire.properties.get("lab").cloned().unwrap_or_default();
        if !label.is_empty() {
            out.nets.insert(label.clone());
        }
        out.wires.push(SemanticWire {
            x1: wire.x1, y1: wire.y1, x2: wire.x2, y2: wire.y2, label,
        });
    }

    for net in netlist.nets.keys() {
        out.nets.insert(net.clone());
    }

    out
}

// ─── Diff ────────────────────────────────────────────────────────────────────

/// Umbral mínimo de similitud para considerar un par (removido, añadido)
/// como renombrado del mismo componente.
const RENAME_THRESHOLD: f64 = 0.8;

fn component_snapshot(c: &SemanticComponent, include_coords: bool) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    out.insert("symbol".to_string(), c.symbol.clone());
    if include_coords {
        out.insert("x".to_string(), c.x.to_string());
        out.insert("y".to_string(), c.y.to_string());
        out.insert("rotation".to_string(), c.rotation.to_string());
        out.insert("mirror".to_string(), c.mirror.to_string());
    }
    for (k, v) in &c.params {
        out.insert(k.clone(), v.clone());
    }
    for (pin, net) in &c.pins {
        out.insert(format!("pin:{pin}"), net.clone());
    }
    out
}

fn param_similarity(a: &SemanticComponent, b: &SemanticComponent) -> f64 {
    if a.symbol != b.symbol { return 0.0; }
    if a.params.is_empty() && b.params.is_empty() { return 1.0; }
    let all_keys: BTreeSet<_> = a.params.keys().chain(b.params.keys()).collect();
    if all_keys.is_empty() { return 1.0; }
    let matching = all_keys.iter()
        .filter(|k| a.params.get(k.as_str()) == b.params.get(k.as_str()))
        .count();
    matching as f64 / all_keys.len() as f64
}

fn detect_renames(
    removed: &[&SemanticComponent],
    added: &[&SemanticComponent],
) -> Vec<(String, String, f64)> {
    let mut candidates: Vec<(usize, usize, f64)> = removed.iter().enumerate()
        .flat_map(|(i, ra)| {
            added.iter().enumerate().filter_map(move |(j, ab)| {
                let sim = param_similarity(ra, ab);
                if sim >= RENAME_THRESHOLD { Some((i, j, sim)) } else { None }
            })
        })
        .collect();
    candidates.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

    let mut renames = Vec::new();
    let mut used_removed = BTreeSet::new();
    let mut used_added = BTreeSet::new();
    for (i, j, sim) in candidates {
        if used_removed.contains(&i) || used_added.contains(&j) { continue; }
        renames.push((removed[i].name.clone(), added[j].name.clone(), sim));
        used_removed.insert(i);
        used_added.insert(j);
    }
    renames
}

/// Compara dos schematics semánticos y devuelve un reporte de cambios.
///
/// Detecta:
/// - Componentes añadidos / removidos / modificados
/// - Renombrados (matching greedy por similitud de parámetros)
/// - Cambios de posición (flag `position_changed`)
/// - Nets añadidas / removidas
/// - Patrón "Move All" (>80% de componentes comunes solo cambiaron de posición)
pub fn diff(sch_a: &SemanticSchematic, sch_b: &SemanticSchematic) -> DiffReport {
    let mut report = DiffReport::default();

    let names_a: BTreeSet<_> = sch_a.components.keys().cloned().collect();
    let names_b: BTreeSet<_> = sch_b.components.keys().cloned().collect();

    let only_a: BTreeSet<_> = names_a.difference(&names_b).cloned().collect();
    let only_b: BTreeSet<_> = names_b.difference(&names_a).cloned().collect();
    let common: BTreeSet<_> = names_a.intersection(&names_b).cloned().collect();

    // ── Renombrados ──────────────────────────────────────────────────────────
    let removed_comps: Vec<_> = only_a.iter()
        .filter_map(|n| sch_a.components.get(n))
        .collect();
    let added_comps: Vec<_> = only_b.iter()
        .filter_map(|n| sch_b.components.get(n))
        .collect();

    let renames = detect_renames(&removed_comps, &added_comps);
    let renamed_from: BTreeSet<_> = renames.iter().map(|(a, _, _)| a.clone()).collect();
    let renamed_to: BTreeSet<_> = renames.iter().map(|(_, b, _)| b.clone()).collect();

    for (name_a, name_b, _) in &renames {
        let ca = &sch_a.components[name_a];
        let cb = &sch_b.components[name_b];
        let mut before = component_snapshot(ca, false);
        let mut after = component_snapshot(cb, false);
        before.insert("name".to_string(), name_a.clone());
        after.insert("name".to_string(), name_b.clone());
        let coords_changed = (ca.x, ca.y, ca.rotation, ca.mirror)
            != (cb.x, cb.y, cb.rotation, cb.mirror);
        report.components.push(ComponentDiff {
            name: format!("{name_a} → {name_b}"),
            kind: ChangeKind::Modified,
            cosmetic: false,
            position_changed: coords_changed,
            before: Some(before),
            after: Some(after),
        });
    }

    // ── Removidos (sin renombrar) ────────────────────────────────────────────
    for name in only_a.iter().filter(|n| !renamed_from.contains(*n)) {
        if let Some(c) = sch_a.components.get(name) {
            report.components.push(ComponentDiff {
                name: name.clone(),
                kind: ChangeKind::Removed,
                cosmetic: false,
                position_changed: false,
                before: Some(component_snapshot(c, false)),
                after: None,
            });
        }
    }

    // ── Añadidos (sin renombrar) ─────────────────────────────────────────────
    for name in only_b.iter().filter(|n| !renamed_to.contains(*n)) {
        if let Some(c) = sch_b.components.get(name) {
            report.components.push(ComponentDiff {
                name: name.clone(),
                kind: ChangeKind::Added,
                cosmetic: false,
                position_changed: false,
                before: None,
                after: Some(component_snapshot(c, false)),
            });
        }
    }

    // ── Modificados / cosméticos ─────────────────────────────────────────────
    let mut coord_only_count = 0usize;
    let mut coord_only_entries = Vec::new();

    for name in &common {
        let ca = &sch_a.components[name];
        let cb = &sch_b.components[name];

        let coords_changed = (ca.x, ca.y, ca.rotation, ca.mirror)
            != (cb.x, cb.y, cb.rotation, cb.mirror);
        let params_changed = ca.params != cb.params || ca.symbol != cb.symbol;

        if params_changed {
            report.components.push(ComponentDiff {
                name: name.clone(),
                kind: ChangeKind::Modified,
                cosmetic: false,
                position_changed: coords_changed,
                before: Some(component_snapshot(ca, false)),
                after: Some(component_snapshot(cb, false)),
            });
        } else if coords_changed {
            coord_only_count += 1;
            coord_only_entries.push(ComponentDiff {
                name: name.clone(),
                kind: ChangeKind::Modified,
                cosmetic: true,
                position_changed: true,
                before: Some(component_snapshot(ca, true)),
                after: Some(component_snapshot(cb, true)),
            });
        }
    }

    report.components.extend(coord_only_entries);

    if !common.is_empty() && coord_only_count as f64 / common.len() as f64 > 0.8 {
        report.is_move_all = true;
    }

    // ── Nets ─────────────────────────────────────────────────────────────────
    report.nets_added = sch_b.nets.difference(&sch_a.nets).cloned().collect();
    report.nets_removed = sch_a.nets.difference(&sch_b.nets).cloned().collect();

    report
}

/// Detecta si el contenido corresponde a un schematic xschem, leyendo el header.
pub fn is_xschem(content: &[u8]) -> bool {
    let header = String::from_utf8_lossy(&content[..content.len().min(240)]);
    header.contains("xschem version=")
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_component(name: &str, symbol: &str, params: &[(&str, &str)]) -> SemanticComponent {
        SemanticComponent {
            name: name.to_string(),
            symbol: symbol.to_string(),
            params: params.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            pins: BTreeMap::new(),
            x: 0.0, y: 0.0, rotation: 0, mirror: 0,
        }
    }

    #[test]
    fn similitud_identico_es_uno() {
        let a = make_component("R1", "res", &[("value", "10k"), ("footprint", "0402")]);
        let b = make_component("R2", "res", &[("value", "10k"), ("footprint", "0402")]);
        assert_eq!(param_similarity(&a, &b), 1.0);
    }

    #[test]
    fn similitud_diferente_simbolo_es_cero() {
        let a = make_component("R1", "res", &[("value", "10k")]);
        let b = make_component("C1", "cap", &[("value", "10k")]);
        assert_eq!(param_similarity(&a, &b), 0.0);
    }

    #[test]
    fn detecta_renombrado_mismo_simbolo_y_valor() {
        let a = make_component("R1", "res", &[("value", "10k")]);
        let b = make_component("R2", "res", &[("value", "10k")]);
        let renames = detect_renames(&[&a], &[&b]);
        assert_eq!(renames.len(), 1);
        assert_eq!(renames[0].0, "R1");
        assert_eq!(renames[0].1, "R2");
    }

    #[test]
    fn no_detecta_renombrado_diferente_simbolo() {
        let a = make_component("R1", "res", &[("value", "10k")]);
        let b = make_component("C1", "cap", &[("value", "10k")]);
        let renames = detect_renames(&[&a], &[&b]);
        assert!(renames.is_empty());
    }

    #[test]
    fn diff_vacio_entre_schematics_iguales() {
        let mut sch = SemanticSchematic::default();
        sch.components.insert("R1".into(), make_component("R1", "res", &[("value", "10k")]));
        let report = diff(&sch, &sch);
        assert!(report.is_empty());
    }

    #[test]
    fn diff_detecta_parametro_cambiado() {
        let mut a = SemanticSchematic::default();
        a.components.insert("R1".into(), make_component("R1", "res", &[("value", "10k")]));
        let mut b = SemanticSchematic::default();
        b.components.insert("R1".into(), make_component("R1", "res", &[("value", "22k")]));

        let report = diff(&a, &b);
        assert_eq!(report.components.len(), 1);
        assert_eq!(report.components[0].kind, ChangeKind::Modified);
        assert!(!report.components[0].cosmetic);
    }
}
