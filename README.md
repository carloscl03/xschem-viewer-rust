# xschem-viewer

A native Rust library and CLI for parsing and rendering [Xschem](https://xschem.sourceforge.io/) schematics (`.sch` / `.sym`) to SVG — **no xschem binary required**.

Originally inspired by the TypeScript implementation at [TinyTapeout/xschem-viewer](https://github.com/TinyTapeout/xschem-viewer). This port brings the same functionality to Rust as a reusable library, enabling native integration into EDA toolchains.

Used as the core rendering and parsing engine by [Riku](https://github.com/riku-chip/riku_chip), a semantic version control system for chip design.

---

## Why this exists

Xschem stores schematics as text files, but rendering them visually has always required the `xschem` application running in a GUI or headless X11 session. This library parses and renders `.sch` files entirely in Rust — no subprocess, no display server, no installation dependency.

---

## Features

| | |
|---|---|
| **PEG parser** | Full `.sch` / `.sym` grammar via `pest`. Handles all Xschem primitive types: `Wire`, `Line`, `Rectangle`, `Arc`, `Polygon`, `Text`, `Component`. |
| **SVG renderer** | Dark and light color themes. Resolves component symbols from local PDK directories. Symbol cache is shared across renders via `Arc<Vec<Object>>` — parse each `.sym` file only once. |
| **Netlist extraction** | Builds an `instances + nets` netlist directly from the parsed schematic, without running a SPICE simulator. |
| **Auto PDK detection** | `with_sym_paths_from_xschemrc()` reads `.xschemrc` from the project directory or `~`, parses `PDK_ROOT`, `PDK`, `XSCHEM_SHAREDIR`, and `XSCHEM_LIBRARY_PATH`, and builds the symbol search path automatically. |
| **Standalone CLI** | `xschem-viewer file.sch [-o out.svg] [--light] [--sym-path /path]` |

---

## Installation

### As a library (Cargo dependency)

```toml
# Cargo.toml
[dependencies]
xschem_viewer = { package = "xschem-viewer", git = "https://github.com/carloscl03/xschem-viewer-rust" }
```

### As a CLI tool

```bash
git clone https://github.com/carloscl03/xschem-viewer-rust
cd xschem-viewer-rust
cargo build --release
# Binary: target/release/xschem-viewer
```

Requires Rust 1.75+. No other dependencies needed.

---

## CLI usage

```bash
# Render to stdout
xschem-viewer schematic.sch

# Render to a file
xschem-viewer schematic.sch -o output.svg

# Light theme
xschem-viewer schematic.sch --light -o output.svg

# Provide PDK symbol paths manually
xschem-viewer schematic.sch \
  --sym-path /headless/pdks/sky130A/libs.tech/xschem \
  --sym-path /foss/tools/xschem/share/xschem_library/devices \
  -o output.svg
```

Missing symbols are reported as warnings on stderr but do not abort the render — they appear as empty placeholders in the SVG.

---

## Library usage

### Quick start — one-shot render

```rust
use xschem_viewer::{RenderOptions, render_svg};

let content = std::fs::read_to_string("schematic.sch")?;

let result = render_svg(&content, RenderOptions::dark())?;
std::fs::write("output.svg", &result.svg)?;

for sym in &result.missing_symbols {
    eprintln!("warning: symbol not found: {sym}");
}
```

### Stateful renderer — shared symbol cache

When rendering multiple schematics from the same PDK, use `Renderer` directly. It keeps a symbol cache across calls, so each `.sym` file is parsed only once:

```rust
use xschem_viewer::{RenderOptions, Renderer};

let opts = RenderOptions::dark()
    .with_sym_paths_from_xschemrc()   // reads .xschemrc automatically
    .with_sym_path("/path/to/extra/symbols");

let mut renderer = Renderer::new(opts);

let result_a = renderer.render(&content_a)?;
let result_b = renderer.render(&content_b)?;  // symbols already cached from A
```

### Netlist extraction

```rust
use xschem_viewer::{parser, extract_netlist};

let schematic = parser::parse(&content)?;
let netlist = extract_netlist(&schematic);

for (name, instance) in &netlist.instances {
    println!("{name:>6}  {}  {:?}", instance.symbol, instance.params);
}

for (net_name, net) in &netlist.nets {
    let pins: Vec<_> = net.pins.iter()
        .map(|p| format!("{}/{}", p.instance, p.pin))
        .collect();
    println!("{net_name}: {}", pins.join(", "));
}
```

---

## Public API reference

### `RenderOptions`

Builder-style configuration for the renderer.

```rust
// Available constructors
RenderOptions::dark()   // Dark background (default)
RenderOptions::light()  // Light background

// Builder methods (chainable)
.with_sym_path(path)              // Add a directory to search for .sym files
.with_sym_paths_from_xschemrc()  // Auto-detect PDK paths from .xschemrc
```

`with_sym_paths_from_xschemrc()` searches for `.xschemrc` in the current directory first, then in `$HOME`. It parses these Tcl assignments:

| Directive | Effect |
|-----------|--------|
| `set PDK_ROOT /path` | Base path for PDK |
| `set PDK sky130A` | PDK name → resolves `$PDK_ROOT/$PDK/libs.tech/xschem` |
| `set XSCHEM_SHAREDIR /path` | Adds `$XSCHEM_SHAREDIR/xschem_library/devices` |
| `append XSCHEM_LIBRARY_PATH :/path` | Adds each colon-separated path |

Only paths that exist on disk are added. The method is a no-op if no `.xschemrc` is found.

---

### `Renderer`

Stateful renderer. Keeps a symbol cache across `render()` calls.

```rust
let mut renderer = Renderer::new(opts);
let result: RenderResult = renderer.render(&content)?;
```

---

### `RenderResult`

```rust
pub struct RenderResult {
    pub svg: String,                    // Complete SVG document
    pub missing_symbols: Vec<String>,   // Symbols that could not be resolved
}
```

---

### `Netlist`

```rust
pub struct Netlist {
    pub instances: BTreeMap<String, Instance>,  // keyed by instance name ("R1", "M3")
    pub nets:      BTreeMap<String, Net>,       // keyed by net name ("Vdd", "GND")
}

pub struct Instance {
    pub name:   String,
    pub symbol: String,                      // e.g. "sky130_fd_pr/nfet_01v8.sym"
    pub params: BTreeMap<String, String>,    // value, W, L, model, …
}

pub struct Net {
    pub name: String,
    pub pins: Vec<Pin>,
}

pub struct Pin {
    pub instance: String,   // which component
    pub pin:      String,   // which pin on that component
}
```

---

## Coordinate system

Schematic coordinates map **directly** to SVG coordinates. Components are rendered as `translate(x, y)` using the parser's `x`/`y` values. The SVG `viewBox` is the bounding box of all rendered primitives — no additional transform layer exists.

This means annotation overlays (such as diff highlights in Riku) can be placed using schematic coordinates without any conversion.

---

## Architecture

```
src/
  lib.rs              — public API surface and re-exports
  main.rs             — standalone CLI binary
  models.rs           — Object, Schematic, Wire, Component, Text, Arc, …
  parser.rs           — pest grammar driver; returns Vec<Object>
  parser/
    xschem.pest       — PEG grammar for .sch and .sym files
    tests.rs          — 21 unit tests (ported from the TypeScript spec)
  netlist.rs          — extract_netlist() + 7 unit tests
  renderer.rs         — Renderer, RenderOptions, junction detection,
                        xschemrc auto-detection, 4 unit tests
```

---

## What is not yet implemented

These are known gaps, not bugs. Pull requests welcome.

| Feature | Notes |
|---------|-------|
| **Pin-to-net topology** | `Net.pins` is populated from label/text matching only. Resolving which physical pin of a symbol connects to which net requires loading the `.sym` file and matching pin positions. |
| **`tcleval(...)` expressions** | Text labels containing Tcl expressions are left as-is in the SVG. |
| **Graph rectangles** | `flags=graph` rectangles are skipped during render. |
| **SPICE / Verilog / VHDL blocks** | Parsed by the grammar but not rendered. |
| **Hierarchical schematics** | Multi-sheet designs (schematic-inside-schematic) are not followed automatically. |

---

## License

[Apache-2.0](LICENSE)
