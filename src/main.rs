use std::path::PathBuf;
use std::process::ExitCode;

use xschem_viewer::{RenderOptions, Renderer};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: xschem-viewer <file.sch> [--output out.svg] [--light] [--sym-path /path ...]");
        return ExitCode::from(1);
    }

    let path = PathBuf::from(&args[1]);
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => { eprintln!("Error reading {path:?}: {e}"); return ExitCode::from(1); }
    };

    let mut output: Option<PathBuf> = None;
    let mut opts = RenderOptions::dark();
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--output" | "-o" => { i += 1; if let Some(p) = args.get(i) { output = Some(p.into()); } }
            "--light" => opts = RenderOptions::light(),
            "--sym-path" => { i += 1; if let Some(p) = args.get(i) { opts = opts.with_sym_path(p); } }
            _ => {}
        }
        i += 1;
    }

    let result = match Renderer::new(opts).render(&content) {
        Ok(r) => r,
        Err(e) => { eprintln!("Error: {e}"); return ExitCode::from(1); }
    };

    for sym in &result.missing_symbols {
        eprintln!("warning: symbol not found: {sym}");
    }

    match output {
        Some(out) => {
            if let Err(e) = std::fs::write(&out, &result.svg) {
                eprintln!("Error writing {out:?}: {e}");
                return ExitCode::from(1);
            }
            eprintln!("SVG written to {out:?}");
        }
        None => print!("{}", result.svg),
    }

    ExitCode::SUCCESS
}
