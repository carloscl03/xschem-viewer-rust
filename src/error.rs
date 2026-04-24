//! Tipos de error públicos de `xschem-viewer`.
//!
//! Devolvemos un enum en lugar de `String` para que los consumidores puedan
//! hacer pattern matching (p. ej. distinguir "no existe el símbolo" de "el
//! parser rechazó el contenido") y recuperarse de forma específica.

use thiserror::Error;

/// Error producido por cualquier operación pública del crate.
#[derive(Debug, Error)]
pub enum XschemError {
    /// El parser pest no pudo leer el contenido del .sch / .sym.
    #[error("parse error: {0}")]
    Parse(String),

    /// No se pudo resolver un símbolo por sus rutas conocidas.
    /// El consumer decide si esto es fatal (muchos schematics toleran
    /// símbolos faltantes renderizándolos como cajas vacías).
    #[error("symbol not found: {0}")]
    SymbolNotFound(String),

    /// Fallo de I/O leyendo un archivo externo (.sym, xschemrc, …).
    #[error("io error on {path}: {source}")]
    Io {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Alias corto para `Result<T, XschemError>`.
pub type Result<T> = std::result::Result<T, XschemError>;
