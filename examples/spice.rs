//! Imprime la netlist SPICE de un esquemático, para compararla con la de
//! Xschem:
//!
//! ```text
//! cargo run --example spice -- [--lvs] [--top] amp.sch [carpeta de símbolos…]
//! ```
//!
//! Busca los símbolos en la carpeta del esquemático, en las que se le pasan,
//! en `$PDK_ROOT/$PDK/libs.tech/xschem` y en las del `xschemrc` de la carpeta
//! actual.

use std::path::{Path, PathBuf};

use xschem_viewer::renderer::RenderOptions;
use xschem_viewer::spice::{netlist, SpiceOptions};

fn main() {
    let mut spice = SpiceOptions::default();
    let mut rest = Vec::new();
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "--lvs" => spice.lvs = true,
            "--top" => spice.top_subckt = true,
            _ => rest.push(a),
        }
    }
    let Some(sch) = rest.first().map(PathBuf::from) else {
        eprintln!("uso: spice [--lvs] [--top] amp.sch [carpeta de símbolos…]");
        std::process::exit(2);
    };
    let text = std::fs::read_to_string(&sch).unwrap_or_else(|e| {
        eprintln!("{}: {e}", sch.display());
        std::process::exit(2);
    });

    let mut dirs: Vec<PathBuf> = vec![sch.parent().unwrap_or(Path::new(".")).to_path_buf()];
    dirs.extend(rest[1..].iter().map(PathBuf::from));
    if let (Ok(root), Ok(pdk)) = (std::env::var("PDK_ROOT"), std::env::var("PDK")) {
        dirs.push(Path::new(&root).join(pdk).join("libs.tech/xschem"));
    }
    let opts = dirs.iter().fold(RenderOptions::dark().with_sym_paths_from_xschemrc(), |o, d| o.with_sym_path(d.clone()));

    // Un sub-esquemático: junto al que lo usa o en las carpetas de símbolos.
    let lookup = |reference: &str, parent: &str| {
        let near = Path::new(parent).parent().unwrap_or(Path::new(".")).join(reference);
        std::iter::once(near)
            .chain(opts.symbol_paths.iter().map(|d| d.join(reference)))
            .find_map(|p| Some((p.to_string_lossy().to_string(), std::fs::read_to_string(&p).ok()?)))
    };
    let name = sch.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    match netlist(&text, &sch.to_string_lossy(), &name, &opts, spice, &lookup) {
        Ok(s) => {
            print!("{}", s.text);
            for w in s.warnings {
                eprintln!("aviso: {w}");
            }
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
