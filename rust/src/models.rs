use std::collections::BTreeMap;

pub type Properties = BTreeMap<String, String>;

#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone)]
pub enum Object {
    Line(Line),
    Rectangle(Rectangle),
    Arc(Arc),
    Text(Text),
    Polygon(Polygon),
    Wire(Wire),
    Component(Component),
    EmbeddedSymbol(Vec<Object>),
    GlobalProperties(Properties),
    Spice(String),
    Verilog(String),
    Spectre(String),
    Vhdl(String),
    Tedax(String),
    Version(Version),
}

#[derive(Debug, Clone)]
pub struct Version {
    pub version: String,
    pub file_version: String,
    pub license: String,
}

#[derive(Debug, Clone)]
pub struct Line {
    pub layer: i32,
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub properties: Properties,
}

#[derive(Debug, Clone)]
pub struct Rectangle {
    pub layer: i32,
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub properties: Properties,
}

#[derive(Debug, Clone)]
pub struct Arc {
    pub layer: i32,
    pub center_x: f64,
    pub center_y: f64,
    pub radius: f64,
    pub start_angle: f64,
    pub sweep_angle: f64,
    pub properties: Properties,
}

#[derive(Debug, Clone)]
pub struct Text {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub rotation: i32,  // 0, 1, 2, 3 → 0°, 90°, 180°, 270°
    pub mirror: i32,    // 0 o 1
    pub h_size: f64,
    pub v_size: f64,
    pub properties: Properties,
}

#[derive(Debug, Clone)]
pub struct Polygon {
    pub layer: i32,
    pub points: Vec<Point>,
    pub properties: Properties,
}

#[derive(Debug, Clone)]
pub struct Wire {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub properties: Properties,
}

#[derive(Debug, Clone)]
pub struct Component {
    pub symbol_reference: String,
    pub x: f64,
    pub y: f64,
    pub rotation: i32,  // 0, 1, 2, 3
    pub flip: i32,      // 0 o 1
    pub properties: Properties,
}

#[derive(Debug, Clone)]
pub struct Schematic {
    pub objects: Vec<Object>,
}

impl Schematic {
    pub fn wires(&self) -> impl Iterator<Item = &Wire> {
        self.objects.iter().filter_map(|o| {
            if let Object::Wire(w) = o { Some(w) } else { None }
        })
    }

    pub fn components(&self) -> impl Iterator<Item = &Component> {
        self.objects.iter().filter_map(|o| {
            if let Object::Component(c) = o { Some(c) } else { None }
        })
    }
}
