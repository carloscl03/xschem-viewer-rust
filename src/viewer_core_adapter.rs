//! Puente entre los tipos internos de `xschem-viewer` y los neutros de
//! `viewer-core`. Solo se compila cuando la feature `viewer-core-compat`
//! está activa.
//!
//! Filosofía: el crate sigue siendo autocontenido y útil sin el adapter;
//! esto es una capa **opcional** para integrarse al ecosistema Riku y ser
//! consumible como `Box<dyn ViewerBackend>` junto a otros backends (GDS,
//! futuro KiCad, …).

use std::sync::Arc;

use async_trait::async_trait;
use viewer_core::{
    backend::{BackendInfo, ViewerBackend},
    bbox::BoundingBox as VcBBox,
    element::{DrawElement as VcDrawElement, HAlign as VcHAlign, Layer as VcLayer, VAlign as VcVAlign},
    error::{Result as VcResult, ViewerError},
    scene::{Scene as VcScene, SceneHandle},
    CancellationToken,
};

use crate::models::{BoundingBox, DrawElement, ResolvedScene};
use crate::renderer::{RenderOptions, Renderer};
use crate::text_layout::{resolve_text_layout, HAlign as XHAlign, VBaseline};

// ─── Conversión de bounding box ──────────────────────────────────────────────

impl From<BoundingBox> for VcBBox {
    fn from(b: BoundingBox) -> Self {
        if b.is_empty() {
            return VcBBox::empty();
        }
        VcBBox {
            min_x: b.min_x,
            min_y: b.min_y,
            max_x: b.max_x,
            max_y: b.max_y,
        }
    }
}

// ─── Conversión de alineación ────────────────────────────────────────────────

fn halign_to_vc(h: XHAlign) -> VcHAlign {
    match h {
        XHAlign::Start => VcHAlign::Start,
        XHAlign::Middle => VcHAlign::Middle,
        XHAlign::End => VcHAlign::End,
    }
}

fn vbaseline_to_vc(v: VBaseline) -> VcVAlign {
    match v {
        VBaseline::Top => VcVAlign::Top,
        VBaseline::Middle => VcVAlign::Middle,
        VBaseline::Bottom => VcVAlign::Bottom,
    }
}

// ─── Conversión de elementos ─────────────────────────────────────────────────

/// Convierte un `DrawElement` interno a uno o cero primitivos neutros.
///
/// - `Arc` se descarta (viewer-core no lo modela — un backend de esquemáticos
///   puede renderizar Arc desde su struct extendida si quiere precisión total).
/// - `MissingSymbol` se convierte en un `Rect` marcador en el layer de texto
///   (capa 3) — suficiente para que un visor genérico sepa "aquí falta algo".
///   Un consumidor Xschem-aware puede ignorar este adapter y usar la escena
///   interna rica.
fn convert_element(el: &DrawElement) -> Option<VcDrawElement> {
    let layer_u16 = |l: i32| -> VcLayer { l.clamp(0, u16::MAX as i32) as u16 };

    Some(match el {
        DrawElement::Line { x1, y1, x2, y2, layer, .. } => VcDrawElement::Line {
            x1: *x1, y1: *y1, x2: *x2, y2: *y2,
            layer: layer_u16(*layer),
        },
        DrawElement::Rect { x, y, w, h, layer, filled, .. } => VcDrawElement::Rect {
            x: *x, y: *y, w: *w, h: *h,
            layer: layer_u16(*layer),
            filled: *filled,
        },
        DrawElement::Circle { cx, cy, r, layer, .. } => VcDrawElement::Circle {
            cx: *cx, cy: *cy, r: *r,
            layer: layer_u16(*layer),
            filled: false,
        },
        DrawElement::Polygon { points, layer, filled, .. } => VcDrawElement::Polygon {
            points: points.clone(),
            layer: layer_u16(*layer),
            filled: *filled,
        },
        DrawElement::Text {
            x, y, content, v_size, rotation, mirror, h_center, v_center, layer, ..
        } => {
            let layout = resolve_text_layout(*rotation, *mirror, *h_center, *v_center);
            VcDrawElement::Text {
                x: *x,
                y: *y,
                content: content.clone(),
                size: *v_size,
                angle_deg: layout.visual_angle_deg as f64,
                h_align: halign_to_vc(layout.h_align),
                v_align: vbaseline_to_vc(layout.baseline),
                layer: layer_u16(*layer),
            }
        }
        DrawElement::Arc { .. } => return None,
        DrawElement::MissingSymbol { x, y, .. } => VcDrawElement::Rect {
            x: *x - 8.0,
            y: *y - 8.0,
            w: 16.0,
            h: 16.0,
            layer: 3,
            filled: false,
        },
    })
}

// ─── Conversión de escena ────────────────────────────────────────────────────

impl From<&ResolvedScene> for VcScene {
    fn from(rs: &ResolvedScene) -> Self {
        let mut scene = VcScene::new();
        for el in &rs.elements {
            if let Some(vc) = convert_element(el) {
                scene.push(vc);
            }
        }
        // La bbox que calcula Scene::push por elemento es consistente; no
        // sobrescribimos con `rs.bbox` para no arrastrar Arc/MissingSymbol
        // descartados o marcadores sintéticos.
        scene
    }
}

// ─── Backend ─────────────────────────────────────────────────────────────────

/// Implementación de `ViewerBackend` para esquemáticos Xschem.
///
/// Sin estado mutable — las opciones viven en un `Arc` para ser compartidas
/// entre tareas async sin clonar `Vec<PathBuf>` por cada carga.
pub struct XschemBackend {
    options: Arc<RenderOptions>,
}

impl XschemBackend {
    /// Opciones `dark()` con auto-detección de xschemrc — el default sensato
    /// para integrarse desde `riku-gui` sin configuración extra.
    pub fn new() -> Self {
        Self::with_options(RenderOptions::dark().with_sym_paths_from_xschemrc())
    }

    pub fn with_options(options: RenderOptions) -> Self {
        Self { options: Arc::new(options) }
    }
}

impl Default for XschemBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ViewerBackend for XschemBackend {
    fn info(&self) -> BackendInfo {
        BackendInfo {
            name: "xschem",
            version: env!("CARGO_PKG_VERSION"),
            extensions: &["sch", "sym"],
        }
    }

    fn accepts(&self, content: &[u8], path_hint: Option<&str>) -> bool {
        if let Some(p) = path_hint {
            let lower = p.to_ascii_lowercase();
            if lower.ends_with(".sch") || lower.ends_with(".sym") {
                return true;
            }
        }
        let head = &content[..content.len().min(240)];
        std::str::from_utf8(head)
            .map(|s| s.contains("xschem version="))
            .unwrap_or(false)
    }

    async fn load(
        &self,
        content: Vec<u8>,
        _path_hint: Option<String>,
        token: CancellationToken,
    ) -> VcResult<SceneHandle> {
        if token.is_cancelled() {
            return Err(ViewerError::Cancelled);
        }
        let opts = Arc::clone(&self.options);
        let handle = tokio::task::spawn_blocking(move || -> VcResult<SceneHandle> {
            let text = std::str::from_utf8(&content)
                .map_err(|e| ViewerError::Parse(e.to_string()))?;

            if token.is_cancelled() {
                return Err(ViewerError::Cancelled);
            }

            let local_opts = RenderOptions {
                colors: opts.colors.clone(),
                symbol_paths: opts.symbol_paths.clone(),
            };
            let renderer = Renderer::new(local_opts);
            let resolved = renderer
                .resolve(text)
                .map_err(|e| ViewerError::Parse(e.to_string()))?;

            if token.is_cancelled() {
                return Err(ViewerError::Cancelled);
            }

            let scene = VcScene::from(&resolved);
            Ok(Arc::new(scene) as SceneHandle)
        })
        .await??;

        Ok(handle)
    }
}
