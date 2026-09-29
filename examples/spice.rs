//! Imprime la netlist SPICE de un esquemático, para compararla con la de
//! Xschem:
//!
//! ```text
//! cargo run --example spice -- [--lvs] [--top] amp.sch [carpeta de símbolos…]
//! ```
//!
//! Busca los símbolos en la carpeta del esquemático, en las que se le pasan,
//! en `$PDK_ROOT/$PDK/libs.tech/xschem` y en las del `xschemrc` de la carpeta
//! actual. Las variables de los `tcleval(…)` salen del `xschemrc` del PDK.

use std::path::{Path, PathBuf};
use std::sync::Arc;

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

    let pdk = std::env::var("PDK_ROOT").ok().zip(std::env::var("PDK").ok()).map(|(r, p)| Path::new(&r).join(p));
    let rc = pdk.as_ref().and_then(|p| std::fs::read_to_string(p.join("libs.tech/xschem/xschemrc")).ok()).unwrap_or_default();
    let env = |n: &str| std::env::var(n.trim_start_matches("env(").trim_end_matches(')')).ok();
    let vars = xschem_viewer::tcleval::rc_vars(&rc, &env);
    spice.vars = Some(Arc::new(move |n: &str| vars.get(n).cloned().or_else(|| env(n))));

    let mut dirs: Vec<PathBuf> = vec![sch.parent().unwrap_or(Path::new(".")).to_path_buf()];
    dirs.extend(rest[1..].iter().map(PathBuf::from));
    if let Some(pdk) = &pdk {
        dirs.push(pdk.join("libs.tech/xschem"));
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
