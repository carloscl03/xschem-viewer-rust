//! Viewport 2D neutral (pan + zoom) y transformación mundo → pantalla.
//!
//! Es matemática pura: no depende de ningún backend gráfico. Cualquier
//! frontend (egui, canvas web, SVG, terminal) puede consumirlo para mapear
//! coordenadas del schematic (unidades xschem) a píxeles de pantalla.
//!
//! ## Convenciones
//!
//! - El eje Y crece **hacia abajo** (igual que xschem, SVG y la mayoría
//!   de sistemas gráficos). No hay inversión interna.
//! - `(pan_x, pan_y)` es el punto del mundo que queda en el centro del
//!   rectángulo de destino.
//! - `scale` es en píxeles-por-unidad-mundo. Duplicar el valor duplica el
//!   zoom visual.
//!
//! ## Ejemplo
//!
//! ```
//! use xschem_viewer::viewport::{Viewport, world_to_screen};
//! use xschem_viewer::BoundingBox;
//!
//! let mut bbox = BoundingBox::default();
//! bbox.expand(0.0, 0.0);
//! bbox.expand(100.0, 100.0);
//!
//! let mut vp = Viewport::default();
//! vp.fit_to(&bbox, 800.0, 600.0);
//!
//! // Un punto del mundo (50,50) cae cerca del centro del rect de destino.
//! let (sx, sy) = world_to_screen(&vp, 800.0, 600.0, 50.0, 50.0);
//! assert!((sx - 400.0).abs() < 1.0);
//! assert!((sy - 300.0).abs() < 1.0);
//! ```

use crate::BoundingBox;

/// Estado de pan + zoom sobre el mundo del schematic.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    /// Punto del mundo que queda en el centro de la región visible.
    pub pan_x: f64,
    pub pan_y: f64,
    /// Píxeles por unidad del mundo. Mayor = zoom in.
    pub scale: f64,
}

impl Default for Viewport {
    fn default() -> Self {
        Self { pan_x: 0.0, pan_y: 0.0, scale: 1.0 }
    }
}

impl Viewport {
    /// Ajusta pan y scale para que el `BoundingBox` quede centrado y
    /// completamente contenido en una región de ancho × alto píxeles,
    /// dejando un margen del 10 %.
    ///
    /// Si el bbox está vacío la operación es un no-op.
    pub fn fit_to(&mut self, bbox: &BoundingBox, width_px: f64, height_px: f64) {
        if bbox.is_empty() { return; }
        let w = bbox.width().max(1.0);
        let h = bbox.height().max(1.0);
        let scale_x = width_px / w;
        let scale_y = height_px / h;
        self.scale = (scale_x.min(scale_y) * 0.9).max(0.001);
        self.pan_x = bbox.min_x + w / 2.0;
        self.pan_y = bbox.min_y + h / 2.0;
    }

    /// Multiplica el zoom preservando el punto del mundo bajo un píxel
    /// específico (zoom centrado en el cursor, útil al hacer scroll).
    ///
    /// `screen_center_x/y` son el centro de la región de dibujo; `cursor_sx/sy`
    /// es la posición del cursor en pantalla. `factor > 1.0` hace zoom in.
    pub fn zoom_at(
        &mut self,
        factor: f64,
        screen_center_x: f64,
        screen_center_y: f64,
        cursor_sx: f64,
        cursor_sy: f64,
    ) {
        let world_before_x = screen_to_world_x(self, screen_center_x, cursor_sx);
        let world_before_y = screen_to_world_y(self, screen_center_y, cursor_sy);
        self.scale = (self.scale * factor).max(1e-6);
        // Ajustar pan para que (world_before_x, world_before_y) siga bajo el cursor.
        self.pan_x = world_before_x - (cursor_sx - screen_center_x) / self.scale;
        self.pan_y = world_before_y - (cursor_sy - screen_center_y) / self.scale;
    }

    /// Desplaza el viewport en unidades del mundo.
    pub fn pan_by_world(&mut self, dx: f64, dy: f64) {
        self.pan_x += dx;
        self.pan_y += dy;
    }

    /// Desplaza el viewport en píxeles de pantalla (convierte internamente).
    pub fn pan_by_screen(&mut self, dpx: f64, dpy: f64) {
        self.pan_x -= dpx / self.scale;
        self.pan_y -= dpy / self.scale;
    }
}

// ─── Transformaciones libres ─────────────────────────────────────────────────

/// Convierte un punto del mundo `(wx, wy)` a coordenadas de pantalla.
///
/// `width_px` y `height_px` son el tamaño de la región de destino.
/// Devuelve `(sx, sy)` como `f32` para facilitar el uso con APIs gráficas.
pub fn world_to_screen(
    vp: &Viewport,
    width_px: f64,
    height_px: f64,
    wx: f64,
    wy: f64,
) -> (f32, f32) {
    let cx = width_px / 2.0;
    let cy = height_px / 2.0;
    let sx = cx + (wx - vp.pan_x) * vp.scale;
    let sy = cy + (wy - vp.pan_y) * vp.scale;
    (sx as f32, sy as f32)
}

/// Convierte un punto de pantalla (absoluto) a coordenadas del mundo.
/// Inverso exacto de `world_to_screen`.
pub fn screen_to_world(
    vp: &Viewport,
    width_px: f64,
    height_px: f64,
    sx: f64,
    sy: f64,
) -> (f64, f64) {
    let cx = width_px / 2.0;
    let cy = height_px / 2.0;
    let wx = vp.pan_x + (sx - cx) / vp.scale;
    let wy = vp.pan_y + (sy - cy) / vp.scale;
    (wx, wy)
}

fn screen_to_world_x(vp: &Viewport, screen_center_x: f64, sx: f64) -> f64 {
    vp.pan_x + (sx - screen_center_x) / vp.scale
}

fn screen_to_world_y(vp: &Viewport, screen_center_y: f64, sy: f64) -> f64 {
    vp.pan_y + (sy - screen_center_y) / vp.scale
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_to_vacio_es_noop() {
        let mut vp = Viewport::default();
        let bbox = BoundingBox::default();
        vp.fit_to(&bbox, 800.0, 600.0);
        assert_eq!(vp, Viewport::default());
    }

    #[test]
    fn fit_to_centra_el_bbox() {
        let mut bbox = BoundingBox::default();
        bbox.expand(0.0, 0.0);
        bbox.expand(100.0, 100.0);

        let mut vp = Viewport::default();
        vp.fit_to(&bbox, 800.0, 600.0);

        assert_eq!(vp.pan_x, 50.0);
        assert_eq!(vp.pan_y, 50.0);
        assert!(vp.scale > 0.0);
    }

    #[test]
    fn world_to_screen_centro_cae_en_medio_pantalla() {
        let vp = Viewport { pan_x: 50.0, pan_y: 50.0, scale: 4.0 };
        let (sx, sy) = world_to_screen(&vp, 800.0, 600.0, 50.0, 50.0);
        assert!((sx - 400.0).abs() < 0.01);
        assert!((sy - 300.0).abs() < 0.01);
    }

    #[test]
    fn screen_to_world_invierte_world_to_screen() {
        let vp = Viewport { pan_x: 10.0, pan_y: -5.0, scale: 2.5 };
        let (sx, sy) = world_to_screen(&vp, 1024.0, 768.0, 123.4, -67.8);
        let (wx, wy) = screen_to_world(&vp, 1024.0, 768.0, sx as f64, sy as f64);
        assert!((wx - 123.4).abs() < 0.001);
        assert!((wy + 67.8).abs() < 0.001);
    }

    #[test]
    fn zoom_at_preserva_punto_bajo_cursor() {
        let mut vp = Viewport { pan_x: 0.0, pan_y: 0.0, scale: 1.0 };
        let (cursor_sx, cursor_sy) = (600.0, 400.0); // fuera del centro
        // Punto del mundo antes del zoom
        let (wx_before, wy_before) = screen_to_world(&vp, 800.0, 600.0, cursor_sx, cursor_sy);
        vp.zoom_at(2.0, 400.0, 300.0, cursor_sx, cursor_sy);
        // El mismo punto del mundo debería seguir bajo el mismo píxel
        let (wx_after, wy_after) = screen_to_world(&vp, 800.0, 600.0, cursor_sx, cursor_sy);
        assert!((wx_after - wx_before).abs() < 0.01);
        assert!((wy_after - wy_before).abs() < 0.01);
    }
}
