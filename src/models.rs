use std::collections::{BTreeMap, HashMap};

pub type Properties = BTreeMap<String, String>;

// ─── Resolved scene ──────────────────────────────────────────────────────────

/// Un primitivo gráfico con transformaciones ya aplicadas, listo para cualquier backend.
/// Las coordenadas están en espacio de mundo del schematic (unidades xschem).
#[derive(Debug, Clone, PartialEq)]
pub enum DrawElement {
    Line {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        layer: i32,
        /// Componente al que pertenece este primitivo, si aplica.
        component_id: Option<String>,
    },
    Rect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        layer: i32,
        filled: bool,
        component_id: Option<String>,
    },
    Circle {
        cx: f64,
        cy: f64,
        r: f64,
        layer: i32,
        component_id: Option<String>,
    },
    Arc {
        cx: f64,
        cy: f64,
        r: f64,
        start_angle: f64,
        sweep_angle: f64,
        layer: i32,
        component_id: Option<String>,
    },
    Polygon {
        points: Vec<(f64, f64)>,
        layer: i32,
        filled: bool,
        component_id: Option<String>,
    },
    Text {
        x: f64,
        y: f64,
        content: String,
        v_size: f64,
        h_size: f64,
        /// 0–3 en pasos de 90°
        rotation: i32,
        /// 0 o 1 — flip horizontal explícito
        mirror: i32,
        h_center: bool,
        v_center: bool,
        layer: i32,
        component_id: Option<String>,
    },
    /// Símbolo que no pudo resolverse. El consumer decide cómo representarlo.
    MissingSymbol {
        name: String,
        x: f64,
        y: f64,
        /// Nombre del componente instanciado (propiedad `name` del .sch).
        component_id: Option<String>,
    },
}

impl DrawElement {
    pub fn component_id(&self) -> Option<&str> {
        match self {
            Self::Line { component_id, .. }
            | Self::Rect { component_id, .. }
            | Self::Circle { component_id, .. }
            | Self::Arc { component_id, .. }
            | Self::Polygon { component_id, .. }
            | Self::Text { component_id, .. }
            | Self::MissingSymbol { component_id, .. } => component_id.as_deref(),
        }
    }

    pub fn layer(&self) -> Option<i32> {
        match self {
            Self::Line { layer, .. }
            | Self::Rect { layer, .. }
            | Self::Circle { layer, .. }
            | Self::Arc { layer, .. }
            | Self::Polygon { layer, .. }
            | Self::Text { layer, .. } => Some(*layer),
            Self::MissingSymbol { .. } => None,
        }
    }
}

/// Escena resuelta: primitivos planos + bounding box, listos para render.
/// Generada por `Renderer::resolve()` — independiente del backend de salida.
#[derive(Debug, Clone)]
pub struct ResolvedScene {
    pub elements: Vec<DrawElement>,
    pub bbox: BoundingBox,
    /// Símbolos que no pudieron resolverse (nombres únicos).
    pub missing_symbols: Vec<String>,
    /// Coordenadas de wires del schematic raíz (x1,y1,x2,y2).
    /// Separados de elements para que el backend pueda calcular junctions
    /// sin distinguir wires de líneas de símbolo por layer.
    pub wires: Vec<(f64, f64, f64, f64)>,
    /// Posiciones de pines en espacio de mundo por instancia.
    /// Clave: nombre de instancia (propiedad `name`).
    /// Valor: lista de (nombre_del_pin, x, y).
    /// Usada para conectar pins a nets por matching geométrico con endpoints de wires.
    pub pin_positions: HashMap<String, Vec<(String, f64, f64)>>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct BoundingBox {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
    initialized: bool,
}

impl BoundingBox {
    pub fn expand(&mut self, x: f64, y: f64) {
        if !self.initialized {
            self.min_x = x;
            self.min_y = y;
            self.max_x = x;
            self.max_y = y;
            self.initialized = true;
        } else {
            self.min_x = self.min_x.min(x);
            self.min_y = self.min_y.min(y);
            self.max_x = self.max_x.max(x);
            self.max_y = self.max_y.max(y);
        }
    }

    pub fn expand_rect(&mut self, x1: f64, y1: f64, x2: f64, y2: f64) {
        self.expand(x1, y1);
        self.expand(x2, y2);
    }

    pub fn width(&self) -> f64 {
        self.max_x - self.min_x
    }

    pub fn height(&self) -> f64 {
        self.max_y - self.min_y
    }

    pub fn is_empty(&self) -> bool {
        !self.initialized
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq)]
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

#[derive(Debug, Clone, PartialEq)]
pub struct Version {
    pub version: String,
    pub file_version: String,
    pub license: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub layer: i32,
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub properties: Properties,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rectangle {
    pub layer: i32,
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub properties: Properties,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Arc {
    pub layer: i32,
    pub center_x: f64,
    pub center_y: f64,
    pub radius: f64,
    pub start_angle: f64,
    pub sweep_angle: f64,
    pub properties: Properties,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub rotation: i32,
    pub mirror: i32,
    pub h_size: f64,
    pub v_size: f64,
    pub properties: Properties,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Polygon {
    pub layer: i32,
    pub points: Vec<Point>,
    pub properties: Properties,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Wire {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub properties: Properties,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Component {
    pub symbol_reference: String,
    pub x: f64,
    pub y: f64,
    pub rotation: i32,
    pub flip: i32,
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
