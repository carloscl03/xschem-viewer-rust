use crate::models::Object;
use crate::parser::parse;

fn only<T>(mut v: Vec<T>) -> T {
    assert_eq!(v.len(), 1);
    v.remove(0)
}

fn objects(input: &str) -> Vec<Object> {
    parse(input).expect("parse failed").objects
}

fn object(input: &str) -> Object {
    only(objects(input))
}

// ─── Version ─────────────────────────────────────────────────────────────────

#[test]
fn empty_file() {
    assert!(objects("").is_empty());
}

#[test]
fn version_basic() {
    let obj = object("v {xschem version=3.4.5 file_version=1.2}");
    let Object::Version(v) = obj else { panic!("expected Version") };
    assert_eq!(v.version, "3.4.5");
    assert_eq!(v.file_version, "1.2");
    assert_eq!(v.license.trim(), "");
}

#[test]
fn version_rc_suffix() {
    let obj = object("v {xschem version=3.4.6RC file_version=1.2}");
    let Object::Version(v) = obj else { panic!("expected Version") };
    assert_eq!(v.version, "3.4.6RC");
}

#[test]
fn version_with_license() {
    let input = "v {xschem version=3.4.5 file_version=1.2\n* Copyright 2021\n}";
    let obj = object(input);
    let Object::Version(v) = obj else { panic!("expected Version") };
    assert!(v.license.contains("Copyright 2021"));
}

// ─── Line ────────────────────────────────────────────────────────────────────

#[test]
fn line_basic() {
    let obj = object("L 4 -50 20 50 20 {dash=5}");
    let Object::Line(l) = obj else { panic!("expected Line") };
    assert_eq!(l.layer, 4);
    assert_eq!(l.x1, -50.0);
    assert_eq!(l.y1, 20.0);
    assert_eq!(l.x2, 50.0);
    assert_eq!(l.y2, 20.0);
    assert_eq!(l.properties.get("dash").map(|s| s.as_str()), Some("5"));
}

// ─── Rectangle ───────────────────────────────────────────────────────────────

#[test]
fn rectangle_basic() {
    let obj = object("B 5 -60 30 60 -30 {fill=true}");
    let Object::Rectangle(r) = obj else { panic!("expected Rectangle") };
    assert_eq!(r.layer, 5);
    assert_eq!(r.x1, -60.0);
    assert_eq!(r.y1, 30.0);
    assert_eq!(r.x2, 60.0);
    assert_eq!(r.y2, -30.0);
    assert_eq!(r.properties.get("fill").map(|s| s.as_str()), Some("true"));
}

// ─── Arc ─────────────────────────────────────────────────────────────────────

#[test]
fn arc_basic() {
    let obj = object("A 4 50 50 25 0 180 {fill=true}");
    let Object::Arc(a) = obj else { panic!("expected Arc") };
    assert_eq!(a.layer, 4);
    assert_eq!(a.center_x, 50.0);
    assert_eq!(a.center_y, 50.0);
    assert_eq!(a.radius, 25.0);
    assert_eq!(a.start_angle, 0.0);
    assert_eq!(a.sweep_angle, 180.0);
    assert_eq!(a.properties.get("fill").map(|s| s.as_str()), Some("true"));
}

// ─── Polygon ─────────────────────────────────────────────────────────────────

#[test]
fn polygon_basic() {
    let obj = object("P 3 4 0 0 100 0 100 100 0 100 {fill=false}");
    let Object::Polygon(p) = obj else { panic!("expected Polygon") };
    assert_eq!(p.layer, 3);
    assert_eq!(p.points.len(), 4);
    assert_eq!(p.points[0].x, 0.0);
    assert_eq!(p.points[0].y, 0.0);
    assert_eq!(p.points[1].x, 100.0);
    assert_eq!(p.points[2].y, 100.0);
    assert_eq!(p.properties.get("fill").map(|s| s.as_str()), Some("false"));
}

// ─── Text ────────────────────────────────────────────────────────────────────

#[test]
fn text_basic() {
    let obj = object("T {Sample Text} 100 150 0 0 1 1 {font=Arial}");
    let Object::Text(t) = obj else { panic!("expected Text") };
    assert_eq!(t.text, "Sample Text");
    assert_eq!(t.x, 100.0);
    assert_eq!(t.y, 150.0);
    assert_eq!(t.rotation, 0);
    assert_eq!(t.mirror, 0);
    assert_eq!(t.h_size, 1.0);
    assert_eq!(t.v_size, 1.0);
    assert_eq!(t.properties.get("font").map(|s| s.as_str()), Some("Arial"));
}

#[test]
fn text_escaped_braces() {
    let obj = object(r"T {hello \{ world \}} 0 0 0 0 1 1 {}");
    let Object::Text(t) = obj else { panic!("expected Text") };
    assert!(t.text.contains('{'));
    assert!(t.text.contains('}'));
}

// ─── Wire ────────────────────────────────────────────────────────────────────

#[test]
fn wire_basic() {
    let obj = object("N 10 10 100 100 {}");
    let Object::Wire(w) = obj else { panic!("expected Wire") };
    assert_eq!(w.x1, 10.0);
    assert_eq!(w.y1, 10.0);
    assert_eq!(w.x2, 100.0);
    assert_eq!(w.y2, 100.0);
    assert!(w.properties.is_empty());
}

// ─── Component ───────────────────────────────────────────────────────────────

#[test]
fn component_basic() {
    let obj = object("C {component.sym} 200 -200 1 0 {name=U1}");
    let Object::Component(c) = obj else { panic!("expected Component") };
    assert_eq!(c.symbol_reference, "component.sym");
    assert_eq!(c.x, 200.0);
    assert_eq!(c.y, -200.0);
    assert_eq!(c.rotation, 1);
    assert_eq!(c.flip, 0);
    assert_eq!(c.properties.get("name").map(|s| s.as_str()), Some("U1"));
}

#[test]
fn component_quoted_property() {
    let obj = object(r#"C {devices/title.sym} 160 -30 0 0 {name=l1 author="Tiny Tapeout"}"#);
    let Object::Component(c) = obj else { panic!("expected Component") };
    assert_eq!(c.symbol_reference, "devices/title.sym");
    assert_eq!(c.properties.get("author").map(|s| s.as_str()), Some("Tiny Tapeout"));
}

#[test]
fn component_multiline_properties() {
    let input = "C {sky130_fd_pr/nfet3_05v0_nvt.sym} 3460 -840 0 0 {name=M20\nL=0.9\nW=1\nbody=GND\n}";
    let obj = object(input);
    let Object::Component(c) = obj else { panic!("expected Component") };
    assert_eq!(c.symbol_reference, "sky130_fd_pr/nfet3_05v0_nvt.sym");
    assert_eq!(c.properties.get("L").map(|s| s.as_str()), Some("0.9"));
    assert_eq!(c.properties.get("body").map(|s| s.as_str()), Some("GND"));
}

#[test]
fn component_escaped_value() {
    let input = "C {devices/res.sym} 1450 -740 1 0 {name=R1\nvalue=\\{2/1.82p/FREQ\\}\nfootprint=1206\n}";
    let obj = object(input);
    let Object::Component(c) = obj else { panic!("expected Component") };
    assert_eq!(c.properties.get("value").map(|s| s.as_str()), Some("{2/1.82p/FREQ}"));
    assert_eq!(c.properties.get("footprint").map(|s| s.as_str()), Some("1206"));
}

// ─── Properties ──────────────────────────────────────────────────────────────

#[test]
fn properties_empty() {
    let obj = object("N 0 0 10 10 {}");
    let Object::Wire(w) = obj else { panic!() };
    assert!(w.properties.is_empty());
}

#[test]
fn properties_multiple() {
    let obj = object("L 4 0 0 10 10 {dash=5 color=red}");
    let Object::Line(l) = obj else { panic!() };
    assert_eq!(l.properties.get("dash").map(|s| s.as_str()), Some("5"));
    assert_eq!(l.properties.get("color").map(|s| s.as_str()), Some("red"));
}

// ─── Multiple objects ─────────────────────────────────────────────────────────

#[test]
fn multiple_objects() {
    let input = "L 4 -50 20 50 20 {dash=5}\nB 5 -60 30 60 -30 {fill=true}";
    let objs = objects(input);
    assert_eq!(objs.len(), 2);
    assert!(matches!(objs[0], Object::Line(_)));
    assert!(matches!(objs[1], Object::Rectangle(_)));
}

// ─── Verilog / Spice ─────────────────────────────────────────────────────────

#[test]
fn verilog_basic() {
    let obj = object("V {assign #1500 LDOUT = LDIN +1;\n}");
    assert!(matches!(obj, Object::Verilog(_)));
}

#[test]
fn spice_basic() {
    let obj = object("S {.param foo=1\n}");
    assert!(matches!(obj, Object::Spice(_)));
}

// ─── Schematic helpers ───────────────────────────────────────────────────────

#[test]
fn schematic_wires_and_components() {
    let input = "N 0 0 10 10 {}\nC {res.sym} 0 0 0 0 {name=R1}";
    let sch = parse(input).unwrap();
    assert_eq!(sch.wires().count(), 1);
    assert_eq!(sch.components().count(), 1);
}
