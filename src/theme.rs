//! Paletas de color públicas y estructuradas.
//!
//! El formato interno de xschem asigna a cada elemento un `layer` (0–21)
//! que es un índice directo en una paleta. Los índices no son arbitrarios:
//! tienen significado consistente entre schematics.
//!
//! En lugar de exponer un `Vec<String>` plano (que obliga a cada consumidor
//! a recordar el mapeo por índice), este módulo ofrece un `Theme` con
//! accesores nombrados para los layers más usados.

/// Índices semánticos conocidos de la paleta xschem.
///
/// Útiles cuando el consumidor quiere un color específico sin depender
/// de literales mágicos.
pub mod layer {
    pub const BACKGROUND: usize = 0;
    pub const WIRE: usize = 1;
    pub const GRID: usize = 2;
    pub const TEXT: usize = 3;
    pub const PIN: usize = 4;
    pub const LABEL: usize = 5;
    pub const COMPONENT: usize = 6;
}

/// Paleta de colores usada por el renderer y por cualquier frontend que
/// quiera mantener coherencia visual con el SVG de referencia.
///
/// Los colores se almacenan como strings hex (`#rrggbb`) — formato nativo
/// del SVG de xschem. Para convertir a otros formatos (RGB bytes, egui
/// Color32, …) el consumidor puede usar cualquier librería de parseo hex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    colors: Vec<String>,
}

impl Theme {
    /// Tema oscuro por defecto — fondo negro, wires cian, texto claro.
    /// Es el equivalente al `dark_theme()` original de xschem.
    pub fn dark() -> Self {
        Self::from_hex_palette(&[
            "#000000","#00ccee","#3f3f3f","#cccccc","#88dd00","#bb2200",
            "#00ccee","#ff0000","#ffff00","#ffffff","#ff00ff","#00ff00",
            "#0000cc","#aaaa00","#aaccaa","#ff7777","#bfff81","#00ffcc",
            "#ce0097","#d2d46b","#ef6158","#fdb200",
        ])
    }

    /// Tema claro — fondo blanco, colores apagados para pantalla.
    pub fn light() -> Self {
        Self::from_hex_palette(&[
            "#ffffff","#0044ee","#aaaaaa","#222222","#229900","#bb2200",
            "#00ccee","#ff0000","#888800","#00aaaa","#880088","#00ff00",
            "#0000cc","#666600","#557755","#aa2222","#7ccc40","#00ffcc",
            "#ce0097","#d2d46b","#ef6158","#fdb200",
        ])
    }

    /// Construye un tema desde una paleta hex arbitraria.
    /// Si hay menos de 22 colores, `layer(n)` para índices altos devolverá
    /// blanco por defecto.
    pub fn from_hex_palette(palette: &[&str]) -> Self {
        Self { colors: palette.iter().map(|s| s.to_string()).collect() }
    }

    /// Construye un tema a partir de un `Vec<String>` pre-existente.
    /// Punto de compatibilidad con código que ya trabajaba con el vector plano.
    pub fn from_vec(colors: Vec<String>) -> Self {
        Self { colors }
    }

    /// Devuelve los colores como slice. Útil para pasar al renderer interno.
    pub fn as_slice(&self) -> &[String] {
        &self.colors
    }

    /// Consume el tema y devuelve la paleta plana.
    pub fn into_vec(self) -> Vec<String> {
        self.colors
    }

    /// Color del layer `n`. Devuelve blanco si el índice está fuera de rango.
    pub fn layer(&self, n: usize) -> &str {
        self.colors.get(n).map(|s| s.as_str()).unwrap_or("#ffffff")
    }

    // ─── Accesores semánticos ────────────────────────────────────────────────

    pub fn background(&self) -> &str { self.layer(layer::BACKGROUND) }
    pub fn wire(&self)       -> &str { self.layer(layer::WIRE) }
    pub fn grid(&self)       -> &str { self.layer(layer::GRID) }
    pub fn text(&self)       -> &str { self.layer(layer::TEXT) }
    pub fn pin(&self)        -> &str { self.layer(layer::PIN) }
    pub fn label(&self)      -> &str { self.layer(layer::LABEL) }
    pub fn component(&self)  -> &str { self.layer(layer::COMPONENT) }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_tiene_fondo_negro() {
        assert_eq!(Theme::dark().background(), "#000000");
    }

    #[test]
    fn light_tiene_fondo_blanco() {
        assert_eq!(Theme::light().background(), "#ffffff");
    }

    #[test]
    fn layer_fuera_de_rango_devuelve_blanco() {
        let t = Theme::dark();
        assert_eq!(t.layer(100), "#ffffff");
    }

    #[test]
    fn accesor_wire_coincide_con_indice_1() {
        let t = Theme::dark();
        assert_eq!(t.wire(), t.layer(1));
    }
}
