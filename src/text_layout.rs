//! Layout de texto con las reglas de legibilidad de xschem.
//!
//! En xschem, el texto NUNCA queda cabeza abajo ni al revés: siempre se lee
//! con la base hacia abajo o hacia la derecha. Cuando la combinación
//! rotation/mirror dejaría el texto invertido, xschem aplica scales extra
//! para volverlo legible. Esta función centraliza esa regla para que todos
//! los backends (SVG, egui, canvas, …) produzcan el mismo resultado visual.
//!
//! El algoritmo replica el comportamiento del renderer original de xschem:
//!
//! ```text
//!   v_mirror = rotation == 1 || rotation == 2
//!   h_mirror = mirror == 1 ? !v_mirror : v_mirror
//! ```
//!
//! - `text_anchor`: inicio, fin o centro, según `h_center` o `h_mirror`.
//! - `baseline`: ascendente, descendente o centro, según `v_center` o
//!   `v_mirror`.
//! - `visual_angle_deg`: el ángulo que debe aplicar el backend (0, −90, 0
//!   o +90 grados) después de considerar el anti-flip.

/// Alineación horizontal equivalente a `text-anchor` de SVG.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HAlign {
    /// Inicio del texto (equivale a `text-anchor="start"`).
    Start,
    /// Centro del texto (equivale a `text-anchor="middle"`).
    Middle,
    /// Fin del texto (equivale a `text-anchor="end"`).
    End,
}

/// Línea base vertical equivalente a `alignment-baseline` de SVG.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VBaseline {
    /// Parte superior del texto (`before-edge`).
    Top,
    /// Centro vertical (`middle`).
    Middle,
    /// Parte inferior (`after-edge`).
    Bottom,
}

/// Dirección en que se apilan las líneas múltiples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineDirection {
    /// De la primera a la última línea (orden natural).
    Forward,
    /// De la última a la primera (se invierte cuando `v_mirror`).
    Reverse,
}

/// Resultado del cálculo de layout de un texto xschem.
///
/// Los campos son datos puros: el consumidor los traduce a su API de
/// render (SVG attributes, egui::Align, etc.) sin tener que reimplementar
/// la regla del anti-flip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextLayout {
    pub h_align: HAlign,
    pub baseline: VBaseline,
    /// Ángulo visual en grados, **positivo = horario**. Ya incluye el
    /// anti-flip: para rotation = 2 devuelve 0 (el texto se ve horizontal
    /// con los anchors volteados, no cabeza abajo).
    pub visual_angle_deg: f32,
    /// Orden en que deben apilarse las líneas cuando `content` tiene
    /// varias (ver `LineDirection`).
    pub line_direction: LineDirection,
    /// Flag auxiliar: true si se aplicó el flip vertical de legibilidad.
    /// Útil para backends que calculan offsets de línea manualmente.
    pub v_mirror: bool,
    /// Flag auxiliar: true si se aplicó el flip horizontal de legibilidad.
    pub h_mirror: bool,
}

/// Resuelve el layout de un texto en función de su `rotation` (0–3 en pasos
/// de 90°), `mirror` (0 o 1), y flags de centrado.
///
/// # Ejemplos
///
/// ```
/// use xschem_viewer::text_layout::{resolve_text_layout, HAlign, VBaseline};
///
/// // Sin rotación ni mirror: texto legible de izquierda a derecha,
/// // anclado por la esquina superior-izquierda.
/// let l = resolve_text_layout(0, 0, false, false);
/// assert_eq!(l.h_align, HAlign::Start);
/// assert_eq!(l.baseline, VBaseline::Top);
/// assert_eq!(l.visual_angle_deg, 0.0);
///
/// // Rotación 180°: se aplica anti-flip, el texto se ve horizontal
/// // pero con anchors invertidos (end + bottom).
/// let l = resolve_text_layout(2, 0, false, false);
/// assert_eq!(l.h_align, HAlign::End);
/// assert_eq!(l.baseline, VBaseline::Bottom);
/// assert_eq!(l.visual_angle_deg, 0.0);
/// ```
pub fn resolve_text_layout(
    rotation: i32,
    mirror: i32,
    h_center: bool,
    v_center: bool,
) -> TextLayout {
    // Reglas base heredadas del renderer original de xschem.
    let v_mirror = rotation == 1 || rotation == 2;
    let h_mirror = if mirror == 1 { !v_mirror } else { v_mirror };

    let h_align = if h_center {
        HAlign::Middle
    } else if h_mirror {
        HAlign::End
    } else {
        HAlign::Start
    };

    let baseline = if v_center {
        VBaseline::Middle
    } else if v_mirror {
        VBaseline::Bottom
    } else {
        VBaseline::Top
    };

    // Ángulo visual después del anti-flip, en grados, horario positivo
    // (convención Y-down usada por SVG y la mayoría de APIs gráficas).
    //   rotation = 0: horizontal normal.
    //   rotation = 1: vertical, base a la derecha (lee hacia arriba)  → -90°.
    //   rotation = 2: horizontal con anchors invertidos               →   0°.
    //   rotation = 3: vertical, base a la izquierda (lee hacia abajo) → +90°.
    let visual_angle_deg = match rotation.rem_euclid(4) {
        0 | 2 => 0.0,
        1 => -90.0,
        3 => 90.0,
        _ => 0.0,
    };

    let line_direction = if v_mirror {
        LineDirection::Reverse
    } else {
        LineDirection::Forward
    };

    TextLayout {
        h_align,
        baseline,
        visual_angle_deg,
        line_direction,
        v_mirror,
        h_mirror,
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotacion_cero_sin_mirror_es_texto_normal() {
        let l = resolve_text_layout(0, 0, false, false);
        assert_eq!(l.h_align, HAlign::Start);
        assert_eq!(l.baseline, VBaseline::Top);
        assert_eq!(l.visual_angle_deg, 0.0);
        assert_eq!(l.line_direction, LineDirection::Forward);
        assert!(!l.v_mirror);
        assert!(!l.h_mirror);
    }

    #[test]
    fn rotacion_90_vertical_base_derecha() {
        let l = resolve_text_layout(1, 0, false, false);
        // v_mirror activo → orden de líneas invertido y baseline bottom
        assert!(l.v_mirror);
        assert!(l.h_mirror);
        assert_eq!(l.line_direction, LineDirection::Reverse);
        assert_eq!(l.visual_angle_deg, -90.0);
    }

    #[test]
    fn rotacion_180_se_ve_horizontal_con_anchors_invertidos() {
        let l = resolve_text_layout(2, 0, false, false);
        // Anti-flip: el ángulo visual es 0 pero los anchors están invertidos
        assert_eq!(l.visual_angle_deg, 0.0);
        assert_eq!(l.h_align, HAlign::End);
        assert_eq!(l.baseline, VBaseline::Bottom);
    }

    #[test]
    fn rotacion_270_vertical_base_izquierda() {
        let l = resolve_text_layout(3, 0, false, false);
        assert!(!l.v_mirror);
        assert!(!l.h_mirror);
        assert_eq!(l.visual_angle_deg, 90.0);
        assert_eq!(l.line_direction, LineDirection::Forward);
    }

    #[test]
    fn mirror_invierte_h_cuando_rotacion_cero() {
        let l = resolve_text_layout(0, 1, false, false);
        assert!(l.h_mirror);
        assert_eq!(l.h_align, HAlign::End);
    }

    #[test]
    fn h_center_y_v_center_sobrescriben_anchors() {
        let l = resolve_text_layout(1, 0, true, true);
        assert_eq!(l.h_align, HAlign::Middle);
        assert_eq!(l.baseline, VBaseline::Middle);
    }
}
