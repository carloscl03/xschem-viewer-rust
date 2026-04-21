use std::path::PathBuf;
use std::process::ExitCode;

mod models;
mod parser;
mod renderer;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: xschem-viewer <file.sch> [--output out.svg] [--light] [--sym-path /path ...]");
        return ExitCode::from(1);
    }

    let path = PathBuf::from(&args[1]);
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error reading {:?}: {e}", path);
            return ExitCode::from(1);
        }
    };

    let mut output: Option<PathBuf> = None;
    let mut light = false;
    let mut sym_paths: Vec<PathBuf> = vec![];
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--output" | "-o" => {
                i += 1;
                if let Some(p) = args.get(i) {
                    output = Some(PathBuf::from(p));
                }
            }
            "--light" => light = true,
            "--sym-path" => {
                i += 1;
                if let Some(p) = args.get(i) {
                    sym_paths.push(PathBuf::from(p));
                }
            }
            _ => {}
        }
        i += 1;
    }

    let schematic = match parser::parse(&content) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Parse error: {e}");
            return ExitCode::from(1);
        }
    };

    let wires = schematic.wires().count();
    let components = schematic.components().count();
    eprintln!("Parsed: {wires} wires, {components} components");

    let mut opts = if light { renderer::RenderOptions::light() } else { renderer::RenderOptions::dark() };
    opts.symbol_paths = sym_paths;

    let svg = renderer::render_to_svg(&schematic, &opts);

    match output {
        Some(out_path) => {
            if let Err(e) = std::fs::write(&out_path, &svg) {
                eprintln!("Error writing {:?}: {e}", out_path);
                return ExitCode::from(1);
            }
            println!("SVG written to {:?}", out_path);
        }
        None => print!("{svg}"),
    }

    ExitCode::SUCCESS
}
