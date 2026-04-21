use std::collections::BTreeMap;

use pest::iterators::Pair;
use pest::Parser;
use pest_derive::Parser;

use crate::models::{
    Arc, Component, Line, Object, Point, Polygon, Properties, Rectangle, Schematic, Text, Version,
    Wire,
};

#[cfg(test)]
mod tests;

#[derive(Parser)]
#[grammar = "parser/xschem.pest"]
struct XschemParser;

pub fn parse(input: &str) -> Result<Schematic, String> {
    let parsed = XschemParser::parse(Rule::schematic, input)
        .map_err(|e| e.to_string())?
        .next()
        .unwrap();

    let mut objects = Vec::new();

    for pair in parsed.into_inner() {
        match pair.as_rule() {
            Rule::version => objects.push(Object::Version(parse_version(pair))),
            Rule::line_obj => objects.push(Object::Line(parse_line(pair))),
            Rule::rectangle_obj => objects.push(Object::Rectangle(parse_rectangle(pair))),
            Rule::arc_obj => objects.push(Object::Arc(parse_arc(pair))),
            Rule::text_obj => objects.push(Object::Text(parse_text(pair))),
            Rule::polygon_obj => objects.push(Object::Polygon(parse_polygon(pair))),
            Rule::wire_obj => objects.push(Object::Wire(parse_wire(pair))),
            Rule::component_obj => objects.push(Object::Component(parse_component(pair))),
            Rule::spice_obj => objects.push(Object::Spice(parse_single_string(pair))),
            Rule::verilog_obj => objects.push(Object::Verilog(parse_single_string(pair))),
            Rule::spectre_obj => objects.push(Object::Spectre(parse_single_string(pair))),
            Rule::vhdl_obj => objects.push(Object::Vhdl(parse_single_string(pair))),
            Rule::tedax_obj => objects.push(Object::Tedax(parse_single_string(pair))),
            Rule::global_props_obj => {
                let props = parse_properties(pair.into_inner().next().unwrap());
                objects.push(Object::GlobalProperties(props));
            }
            Rule::embedded_symbol => {
                let inner = parse_embedded(pair);
                objects.push(Object::EmbeddedSymbol(inner));
            }
            Rule::EOI => {}
            _ => {}
        }
    }

    Ok(Schematic { objects })
}

fn parse_version(pair: Pair<Rule>) -> Version {
    let mut inner = pair.into_inner();
    let version = inner.next().unwrap().as_str().to_string();
    let file_version = inner.next().unwrap().as_str().to_string();
    let license = inner.next().map(|p| p.as_str().trim().to_string()).unwrap_or_default();
    Version { version, file_version, license }
}

fn parse_line(pair: Pair<Rule>) -> Line {
    let mut inner = pair.into_inner();
    let layer = next_int(&mut inner);
    let x1 = next_float(&mut inner);
    let y1 = next_float(&mut inner);
    let x2 = next_float(&mut inner);
    let y2 = next_float(&mut inner);
    let properties = parse_properties(inner.next().unwrap());
    Line { layer, x1, y1, x2, y2, properties }
}

fn parse_rectangle(pair: Pair<Rule>) -> Rectangle {
    let mut inner = pair.into_inner();
    let layer = next_int(&mut inner);
    let x1 = next_float(&mut inner);
    let y1 = next_float(&mut inner);
    let x2 = next_float(&mut inner);
    let y2 = next_float(&mut inner);
    let properties = parse_properties(inner.next().unwrap());
    Rectangle { layer, x1, y1, x2, y2, properties }
}

fn parse_arc(pair: Pair<Rule>) -> Arc {
    let mut inner = pair.into_inner();
    let layer = next_int(&mut inner);
    let center_x = next_float(&mut inner);
    let center_y = next_float(&mut inner);
    let radius = next_float(&mut inner);
    let start_angle = next_float(&mut inner);
    let sweep_angle = next_float(&mut inner);
    let properties = parse_properties(inner.next().unwrap());
    Arc { layer, center_x, center_y, radius, start_angle, sweep_angle, properties }
}

fn parse_text(pair: Pair<Rule>) -> Text {
    let mut inner = pair.into_inner();
    let text = parse_curly_string(inner.next().unwrap());
    let x = next_float(&mut inner);
    let y = next_float(&mut inner);
    let rotation = next_int(&mut inner);
    let mirror = next_int(&mut inner);
    let h_size = next_float(&mut inner);
    let v_size = next_float(&mut inner);
    let properties = parse_properties(inner.next().unwrap());
    Text { text, x, y, rotation, mirror, h_size, v_size, properties }
}

fn parse_polygon(pair: Pair<Rule>) -> Polygon {
    let mut inner = pair.into_inner();
    let layer = next_int(&mut inner);
    let _count = next_int(&mut inner);
    let mut points = Vec::new();
    let mut props = None;
    for p in inner {
        match p.as_rule() {
            Rule::coordinate_pair => {
                let mut coords = p.into_inner();
                let x = coords.next().unwrap().as_str().parse().unwrap_or(0.0);
                let y = coords.next().unwrap().as_str().parse().unwrap_or(0.0);
                points.push(Point { x, y });
            }
            Rule::properties => {
                props = Some(parse_properties(p));
            }
            _ => {}
        }
    }
    Polygon { layer, points, properties: props.unwrap_or_default() }
}

fn parse_wire(pair: Pair<Rule>) -> Wire {
    let mut inner = pair.into_inner();
    let x1 = next_float(&mut inner);
    let y1 = next_float(&mut inner);
    let x2 = next_float(&mut inner);
    let y2 = next_float(&mut inner);
    let properties = parse_properties(inner.next().unwrap());
    Wire { x1, y1, x2, y2, properties }
}

fn parse_component(pair: Pair<Rule>) -> Component {
    let mut inner = pair.into_inner();
    let symbol_reference = parse_curly_string(inner.next().unwrap());
    let x = next_float(&mut inner);
    let y = next_float(&mut inner);
    let rotation = next_int(&mut inner);
    let flip = next_int(&mut inner);
    let properties = parse_properties(inner.next().unwrap());
    Component { symbol_reference, x, y, rotation, flip, properties }
}

fn parse_single_string(pair: Pair<Rule>) -> String {
    parse_curly_string(pair.into_inner().next().unwrap())
}

fn parse_curly_string(pair: Pair<Rule>) -> String {
    pair.into_inner().next().map(|p| p.as_str().to_string()).unwrap_or_default()
}

fn parse_embedded(pair: Pair<Rule>) -> Vec<Object> {
    let mut objects = Vec::new();
    for p in pair.into_inner() {
        match p.as_rule() {
            Rule::version => objects.push(Object::Version(parse_version(p))),
            Rule::line_obj => objects.push(Object::Line(parse_line(p))),
            Rule::rectangle_obj => objects.push(Object::Rectangle(parse_rectangle(p))),
            Rule::arc_obj => objects.push(Object::Arc(parse_arc(p))),
            Rule::text_obj => objects.push(Object::Text(parse_text(p))),
            Rule::polygon_obj => objects.push(Object::Polygon(parse_polygon(p))),
            Rule::wire_obj => objects.push(Object::Wire(parse_wire(p))),
            Rule::component_obj => objects.push(Object::Component(parse_component(p))),
            Rule::spice_obj => objects.push(Object::Spice(parse_single_string(p))),
            Rule::verilog_obj => objects.push(Object::Verilog(parse_single_string(p))),
            Rule::spectre_obj => objects.push(Object::Spectre(parse_single_string(p))),
            Rule::vhdl_obj => objects.push(Object::Vhdl(parse_single_string(p))),
            Rule::tedax_obj => objects.push(Object::Tedax(parse_single_string(p))),
            _ => {}
        }
    }
    objects
}

fn parse_properties(pair: Pair<Rule>) -> Properties {
    let mut map = BTreeMap::new();
    for p in pair.into_inner() {
        if p.as_rule() == Rule::pair {
            let mut kv = p.into_inner();
            if let Some(key_pair) = kv.next() {
                let key = key_pair.as_str().to_string();
                let value = kv
                    .next()
                    .map(|vp| {
                        let inner = vp.into_inner().next().unwrap();
                        unescape(inner.as_str())
                    })
                    .unwrap_or_default();
                if !key.is_empty() {
                    map.insert(key, value);
                }
            }
        }
    }
    map
}

fn unescape(s: &str) -> String {
    let s = if s.starts_with('"') && s.ends_with('"') {
        &s[1..s.len() - 1]
    } else {
        s
    };
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(&next) = chars.peek() {
                out.push(next);
                chars.next();
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn next_float(inner: &mut pest::iterators::Pairs<Rule>) -> f64 {
    inner
        .next()
        .and_then(|p| {
            let s = p.as_str();
            // coordinate wraps float; float wraps the text directly
            if p.as_rule() == Rule::coordinate || p.as_rule() == Rule::float {
                s.parse().ok()
            } else {
                p.into_inner().next().and_then(|f| f.as_str().parse().ok())
            }
        })
        .unwrap_or(0.0)
}

fn next_int(inner: &mut pest::iterators::Pairs<Rule>) -> i32 {
    inner
        .next()
        .and_then(|p| p.as_str().parse().ok())
        .unwrap_or(0)
}
