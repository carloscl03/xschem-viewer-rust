//! `tcleval(...)` en los textos de un símbolo.
//!
//! Xschem evalúa como Tcl el contenido de `tcleval(...)` al dibujar. Los PDK
//! lo usan para mostrar valores calculados, p. ej. la capacitancia de un
//! `cap_mim_m3_1` de sky130:
//!
//! ```text
//! tcleval(C=[to_eng [ev \{1 * (1 * 1 * 2e-15 + ( 1 + 1 ) * 0.38e-15)\}]])  →  C=2.76f
//! ```
//!
//! No hay un intérprete de Tcl: se resuelven las sustituciones `[...]` de
//! los comandos que usan los símbolos (`ev`, `expr`, `to_eng`) con
//! aritmética simple, y las variables (`$::180MCU_MODELS`) si se sabe su
//! valor. Lo que no se puede evaluar queda como `?`, para no mostrar código
//! Tcl en el lienzo.

use std::collections::HashMap;

/// El valor de una variable de Tcl (`SKYWATER_MODELS`, `env(PDK_ROOT)`).
pub type Vars<'a> = &'a dyn Fn(&str) -> Option<String>;

/// Resuelve cada `tcleval(...)` del texto; el resto queda igual.
pub fn eval_text(text: &str) -> String {
    eval_text_with(text, &|_| None)
}

/// Como [`eval_text`], con las variables de `vars` (y `$env(X)` del
/// entorno).
pub fn eval_text_with(text: &str, vars: Vars<'_>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("tcleval(") {
        out.push_str(&rest[..i]);
        let body_start = i + "tcleval(".len();
        match matching(&rest[body_start..], '(', ')') {
            Some(len) => {
                out.push_str(&substitute(&variables(&rest[body_start..body_start + len], vars)));
                rest = &rest[body_start + len + 1..];
            }
            None => {
                out.push_str(&rest[i..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Sustitución de variables: `$::X`, `$X`, `${X}`, `$env(X)`. Las que no
/// se conocen quedan como están.
pub fn variables(s: &str, vars: Vars<'_>) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '$' || (i > 0 && chars[i - 1] == '\\') {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let mut j = i + 1;
        if chars[j..].starts_with(&[':', ':']) {
            j += 2;
        }
        let (name, end) = if chars.get(j) == Some(&'{') {
            match chars[j + 1..].iter().position(|&c| c == '}') {
                Some(k) => (chars[j + 1..j + 1 + k].iter().collect::<String>(), j + 2 + k),
                None => (String::new(), j),
            }
        } else {
            let mut k = j;
            while k < chars.len() && (chars[k].is_alphanumeric() || chars[k] == '_') {
                k += 1;
            }
            let mut name: String = chars[j..k].iter().collect();
            if name == "env" && chars.get(k) == Some(&'(') {
                if let Some(p) = chars[k + 1..].iter().position(|&c| c == ')') {
                    name = format!("env({})", chars[k + 1..k + 1 + p].iter().collect::<String>());
                    k += 2 + p;
                }
            }
            (name, k)
        };
        let value = (!name.is_empty()).then(|| vars(&name)).flatten().or_else(|| {
            let var = name.strip_prefix("env(")?.strip_suffix(')')?;
            std::env::var(var).ok()
        });
        match value {
            Some(v) => {
                out.push_str(&v);
                i = end;
            }
            None => {
                out.push('$');
                i += 1;
            }
        }
    }
    out
}

/// Las variables que fija un `xschemrc` con `set NOMBRE valor`, con las
/// anteriores y las de `env` ya sustituidas. No evalúa los `if`: vale el
/// último `set` que se pudo resolver entero.
pub fn rc_vars(rc: &str, env: Vars<'_>) -> HashMap<String, String> {
    let mut vars: HashMap<String, String> = HashMap::new();
    for line in rc.lines() {
        let Some(rest) = line.trim().strip_prefix("set ") else { continue };
        let Some((name, value)) = rest.trim().split_once(char::is_whitespace) else { continue };
        let (name, value) = (name.trim_start_matches("::"), value.trim());
        // `{…}`: literal; `"…"` o suelto: con sustitución; `[…]`: un comando.
        let resolved = if let Some(v) = value.strip_prefix('{').and_then(|v| v.strip_suffix('}')) {
            v.to_string()
        } else if value.starts_with('[') {
            continue;
        } else {
            let v = value.trim_matches('"');
            variables(v, &|n| vars.get(n).cloned().or_else(|| env(n)))
        };
        if !resolved.contains('$') && !resolved.contains('[') {
            vars.insert(name.to_string(), resolved);
        }
    }
    vars
}

/// Largo hasta el cierre que equilibra `open`/`close` (sin incluirlo).
fn matching(s: &str, open: char, close: char) -> Option<usize> {
    let mut depth = 0;
    for (i, c) in s.char_indices() {
        if c == open {
            depth += 1;
        } else if c == close {
            if depth == 0 {
                return Some(i);
            }
            depth -= 1;
        }
    }
    None
}

/// Sustitución de comandos de Tcl: cada `[cmd args]` por su resultado.
fn substitute(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('[') {
        out.push_str(&rest[..i]);
        match matching(&rest[i + 1..], '[', ']') {
            Some(len) => {
                let cmd = &rest[i + 1..i + 1 + len];
                out.push_str(&command(cmd).unwrap_or_else(|| "?".to_string()));
                rest = &rest[i + 2 + len..];
            }
            None => {
                out.push('?');
                rest = "";
            }
        }
    }
    out.push_str(rest);
    // Llaves escapadas del archivo (`\{`) sin comando alrededor.
    out.replace("\\{", "{").replace("\\}", "}")
}

/// Un comando: `ev {…}`, `expr {…}`, `to_eng x`.
fn command(cmd: &str) -> Option<String> {
    let cmd = cmd.trim();
    let (name, arg) = cmd.split_once(char::is_whitespace).unwrap_or((cmd, ""));
    let arg = unbrace(&substitute(arg.trim()));
    match name {
        "ev" | "ev7" | "expr" => arith(&arg).map(format_number),
        "to_eng" => arith(&arg).map(to_eng),
        _ => None,
    }
}

/// Quita un par de llaves envolventes (`{…}` o `\{…\}`).
fn unbrace(s: &str) -> String {
    let t = s.trim().replace("\\{", "{").replace("\\}", "}");
    match t.strip_prefix('{').and_then(|x| x.strip_suffix('}')) {
        Some(inner) => inner.to_string(),
        None => t,
    }
}

/// Como `%g` de Tcl: 6 cifras significativas, en notación científica si el
/// número es muy chico o muy grande (`2.76e-15`), que también se vuelve a
/// leer bien si el resultado entra en otro comando.
fn format_number(v: f64) -> String {
    if v == 0.0 || !v.is_finite() {
        return "0".into();
    }
    let exp = v.abs().log10().floor() as i32;
    let trim = |s: String| {
        if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
    };
    if !(-4..6).contains(&exp) {
        let m = trim(format!("{:.5}", v / 10f64.powi(exp)));
        format!("{m}e{exp}")
    } else {
        trim(format!("{v:.prec$}", prec = (5 - exp).max(0) as usize))
    }
}

/// Como `to_eng` de Xschem: mantisa con hasta 4 cifras y prefijo (`2.76f`).
pub fn to_eng(v: f64) -> String {
    if v == 0.0 || !v.is_finite() {
        return format_number(v);
    }
    const PREFIXES: [(i32, &str); 11] =
        [(12, "T"), (9, "G"), (6, "M"), (3, "k"), (0, ""), (-3, "m"), (-6, "u"), (-9, "n"), (-12, "p"), (-15, "f"), (-18, "a")];
    let exp = ((v.abs().log10() / 3.0).floor() as i32 * 3).clamp(-18, 12);
    let prefix = PREFIXES.iter().find(|(e, _)| *e == exp).map_or("", |(_, p)| *p);
    let m = v / 10f64.powi(exp);
    let digits = (3 - m.abs().log10().floor() as i32).clamp(0, 3) as usize;
    let s = format!("{m:.digits$}");
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
    format!("{s}{prefix}")
}

// ─── Aritmética de `expr` ──────────────────────────────────────────────────

fn arith(s: &str) -> Option<f64> {
    let mut p = Arith { s: s.as_bytes(), i: 0 };
    let v = p.expr()?;
    p.ws();
    (p.i == p.s.len() && v.is_finite()).then_some(v)
}

struct Arith<'a> {
    s: &'a [u8],
    i: usize,
}

impl Arith<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.i).copied()
    }

    fn expr(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        while let Some(op @ (b'+' | b'-')) = self.peek() {
            self.i += 1;
            let r = self.term()?;
            v = if op == b'+' { v + r } else { v - r };
        }
        Some(v)
    }

    fn term(&mut self) -> Option<f64> {
        let mut v = self.power()?;
        loop {
            match self.peek() {
                Some(b'*') if self.s.get(self.i + 1) != Some(&b'*') => {
                    self.i += 1;
                    v *= self.power()?;
                }
                Some(b'/') => {
                    self.i += 1;
                    v /= self.power()?;
                }
                _ => return Some(v),
            }
        }
    }

    fn power(&mut self) -> Option<f64> {
        let base = self.unary()?;
        if self.peek() == Some(b'*') && self.s.get(self.i + 1) == Some(&b'*') {
            self.i += 2;
            return Some(base.powf(self.power()?));
        }
        Some(base)
    }

    fn unary(&mut self) -> Option<f64> {
        match self.peek()? {
            b'-' => {
                self.i += 1;
                Some(-self.unary()?)
            }
            b'+' => {
                self.i += 1;
                self.unary()
            }
            b'(' => {
                self.i += 1;
                let v = self.expr()?;
                (self.peek()? == b')').then(|| self.i += 1)?;
                Some(v)
            }
            c if c.is_ascii_alphabetic() => self.function(),
            _ => self.number(),
        }
    }

    fn function(&mut self) -> Option<f64> {
        let start = self.i;
        while self.i < self.s.len() && self.s[self.i].is_ascii_alphanumeric() {
            self.i += 1;
        }
        let name = std::str::from_utf8(&self.s[start..self.i]).ok()?.to_string();
        (self.peek()? == b'(').then(|| self.i += 1)?;
        let mut args = vec![self.expr()?];
        while self.peek()? == b',' {
            self.i += 1;
            args.push(self.expr()?);
        }
        (self.peek()? == b')').then(|| self.i += 1)?;
        let a = args[0];
        Some(match (name.as_str(), args.len()) {
            ("sqrt", 1) => a.sqrt(),
            ("abs", 1) => a.abs(),
            ("double", 1) => a,
            ("int", 1) => a.trunc(),
            ("round", 1) => a.round(),
            ("log10", 1) => a.log10(),
            ("log", 1) => a.ln(),
            ("exp", 1) => a.exp(),
            ("pow", 2) => a.powf(args[1]),
            ("max", _) => args.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            ("min", _) => args.iter().copied().fold(f64::INFINITY, f64::min),
            _ => return None,
        })
    }

    fn number(&mut self) -> Option<f64> {
        self.ws();
        let start = self.i;
        while self.i < self.s.len() && (self.s[self.i].is_ascii_digit() || self.s[self.i] == b'.') {
            self.i += 1;
        }
        if self.i < self.s.len() && matches!(self.s[self.i], b'e' | b'E') {
            let mut j = self.i + 1;
            if j < self.s.len() && matches!(self.s[j], b'+' | b'-') {
                j += 1;
            }
            if j < self.s.len() && self.s[j].is_ascii_digit() {
                while j < self.s.len() && self.s[j].is_ascii_digit() {
                    j += 1;
                }
                self.i = j;
            }
        }
        let v: f64 = std::str::from_utf8(&self.s[start..self.i]).ok()?.parse().ok()?;
        // Sufijo SPICE, como el `ev` de Xschem (`1.0u`, `2meg`, `10k`).
        let letters_start = self.i;
        while self.i < self.s.len() && self.s[self.i].is_ascii_alphabetic() {
            self.i += 1;
        }
        let suffix = std::str::from_utf8(&self.s[letters_start..self.i]).ok()?.to_ascii_lowercase();
        let mult = if suffix.starts_with("meg") {
            1e6
        } else {
            match suffix.chars().next() {
                None => 1.0,
                Some('t') => 1e12,
                Some('g') => 1e9,
                Some('k') => 1e3,
                Some('m') => 1e-3,
                Some('u') => 1e-6,
                Some('n') => 1e-9,
                Some('p') => 1e-12,
                Some('f') => 1e-15,
                Some('a') => 1e-18,
                // Otra palabra pegada al número: no es un número.
                _ => return None,
            }
        };
        Some(v * mult)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sky130_mim_capacitance() {
        let t = "tcleval(C=[to_eng [ev \\{1 * (1 * 1 * 2e-15 + ( 1 + 1 ) * 0.38e-15)\\}]])";
        assert_eq!(eval_text(t), "C=2.76f");
    }

    #[test]
    fn engineering_format() {
        assert_eq!(to_eng(2.76e-15), "2.76f");
        assert_eq!(to_eng(1500.0), "1.5k");
        assert_eq!(to_eng(0.00047), "470u");
        assert_eq!(to_eng(12.0), "12");
    }

    #[test]
    fn expr_and_unknown_commands() {
        assert_eq!(eval_text("tcleval(W=[expr {2*0.42}])"), "W=0.84");
        assert_eq!(eval_text("tcleval(x=[foo bar])"), "x=?");
        assert_eq!(eval_text("sin tcleval"), "sin tcleval");
        assert_eq!(eval_text("a tcleval(b) c"), "a b c");
        assert_eq!(eval_text("tcleval(p=[ev \\{2**3\\}])"), "p=8");
        // IHP: valores con sufijo SPICE y `ev7`.
        assert_eq!(eval_text("tcleval(A=[ev7 \\{ 1.0u * 2u \\}])"), "A=2e-12");
        assert_eq!(eval_text("tcleval(C=[to_eng [ev \\{(10u * 10u * 1.5e-3)\\}]])"), "C=150f");
    }

    #[test]
    fn variables_del_xschemrc() {
        let rc = "if {1} {\n  set PDK_ROOT $env(PDK_ROOT)\n  set 180MCU_MODELS ${PDK_ROOT}/gf180mcuD/libs.tech/ngspice\n}\nset raro [pwd]\n";
        let env = |n: &str| (n == "env(PDK_ROOT)").then(|| "/pdks".to_string());
        let vars = rc_vars(rc, &env);
        assert_eq!(vars.get("180MCU_MODELS").map(String::as_str), Some("/pdks/gf180mcuD/libs.tech/ngspice"));
        assert!(!vars.contains_key("raro"));
        let get = |n: &str| vars.get(n).cloned();
        assert_eq!(
            eval_text_with("tcleval(.include $::180MCU_MODELS/design.ngspice $::NADA)", &get),
            ".include /pdks/gf180mcuD/libs.tech/ngspice/design.ngspice $::NADA"
        );
    }
}
