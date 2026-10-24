use crate::render::badges;
use gtk4::gdk;
use gtk4::gdk::subclass::prelude::*;
use gtk4::prelude::*;
use gtk4::{glib, graphene, gsk};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

#[derive(Clone)]
pub(crate) struct RawShape {
    pub d: String,
    pub fill: Option<String>,
    pub stroke: Option<String>,
    pub sw: f64,
    pub cap: String,
    pub join: String,
}

#[derive(Clone)]
struct Ctx {
    fill: Option<String>,
    stroke: Option<String>,
    sw: f64,
    cap: String,
    join: String,
}
impl Default for Ctx {
    fn default() -> Self {
        Self {
            fill: None,
            stroke: None,
            sw: 1.0,
            cap: "butt".into(),
            join: "miter".into(),
        }
    }
}

fn attr(attrs: &[(String, String)], k: &str) -> Option<String> {
    attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
}
fn num(attrs: &[(String, String)], k: &str, d: f64) -> f64 {
    attr(attrs, k)
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(d)
}

fn tag_name(s: &str) -> &str {
    let end = s
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == ':'))
        .unwrap_or(s.len());
    &s[..end]
}

type Attrs = Vec<(String, String)>;
type ParsedIcon = (f32, Vec<RawShape>);

fn read_attrs(text: &str, at: usize) -> Option<(Attrs, bool, usize)> {
    let b = text.as_bytes();
    let mut i = at;
    let mut out: Vec<(String, String)> = Vec::new();
    loop {
        if i >= b.len() {
            return None;
        }
        match b[i] {
            b'>' => return Some((out, false, i + 1)),
            b'/' if b.get(i + 1) == Some(&b'>') => return Some((out, true, i + 2)),
            c if c.is_ascii_whitespace() => i += 1,
            _ => {
                let nstart = i;
                while i < b.len()
                    && ((b[i] as char).is_ascii_alphanumeric()
                        || b[i] as char == '-'
                        || b[i] as char == ':')
                {
                    i += 1;
                }
                if i == nstart {
                    return None;
                }
                let name = text[nstart..i].to_string();
                while i < b.len() && (b[i] as char).is_ascii_whitespace() {
                    i += 1;
                }
                if b.get(i) == Some(&b'=') {
                    i += 1;
                    while i < b.len() && (b[i] as char).is_ascii_whitespace() {
                        i += 1;
                    }
                    let q = *b.get(i)?;
                    let vstart = i.checked_add(1)?;
                    let rel = text[vstart..].find(q as char)?;
                    out.push((name, text[vstart..vstart + rel].to_string()));
                    i = vstart + rel + 1;
                } else {
                    out.push((name, String::new()));
                }
            }
        }
    }
}

pub(crate) fn walk(text: &str) -> Option<(f32, Vec<RawShape>)> {
    let b = text.as_bytes();
    let n = b.len();
    let mut shapes: Vec<RawShape> = Vec::new();
    let mut ctx: Vec<Ctx> = vec![Ctx::default()];
    let mut open: Vec<(String, bool)> = Vec::new();
    let mut units = 16.0f32;
    let mut i = 0usize;
    while i < n {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        if text[i..].starts_with("<!--") {
            i += 4;
            let p = text[i..].find("-->")?;
            i += p + 3;
            continue;
        }
        if text[i..].starts_with("<?") {
            let p = text[i..].find("?>")?;
            i += p + 2;
            continue;
        }
        if text[i..].starts_with("</") {
            let p = text[i..].find('>')?;
            let name = tag_name(&text[i + 2..i + p]).to_string();
            while let Some((tag, pushed)) = open.pop() {
                if pushed {
                    ctx.pop();
                }
                if tag == name {
                    break;
                }
            }
            i += p + 1;
            continue;
        }
        let name = tag_name(&text[i + 1..]).to_string();
        let (attrs, _self_closing, ni) = read_attrs(text, i + 1 + name.len())?;
        i = ni;
        let get = |k: &str| attr(&attrs, k);
        let top = ctx.last().cloned().unwrap_or_default();
        let merged = Ctx {
            fill: get("fill").or_else(|| top.fill.clone()),
            stroke: get("stroke").or_else(|| top.stroke.clone()),
            sw: get("stroke-width")
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(top.sw),
            cap: get("stroke-linecap").unwrap_or_else(|| top.cap.clone()),
            join: get("stroke-linejoin").unwrap_or_else(|| top.join.clone()),
        };
        let group = match name.as_str() {
            "svg" if open.is_empty() => {
                let vb: Vec<f64> = get("viewBox")
                    .unwrap_or_default()
                    .replace(',', " ")
                    .split(' ')
                    .filter(|p| !p.is_empty())
                    .filter_map(|p| p.trim().parse().ok())
                    .collect();
                if vb.len() == 4 {
                    units = vb[2] as f32;
                } else if let Some(w) = get("width") {
                    units = w.trim().parse().unwrap_or(16.0);
                }
                true
            }
            "path" => {
                if let Some(d) = get("d").filter(|d| !d.trim().is_empty()) {
                    shapes.push(RawShape {
                        d,
                        fill: merged.fill.clone(),
                        stroke: merged.stroke.clone(),
                        sw: merged.sw,
                        cap: merged.cap.clone(),
                        join: merged.join.clone(),
                    });
                }
                false
            }
            "rect" => {
                let (x, y) = (num(&attrs, "x", 0.0), num(&attrs, "y", 0.0));
                let (w, h) = (num(&attrs, "width", 0.0), num(&attrs, "height", 0.0));
                let rx = get("rx")
                    .and_then(|v| v.trim().parse::<f64>().ok())
                    .filter(|v| *v != 0.0)
                    .unwrap_or_else(|| num(&attrs, "ry", 0.0));
                shapes.push(RawShape {
                    d: rrect_d(x, y, w, h, rx),
                    fill: merged.fill.clone(),
                    stroke: merged.stroke.clone(),
                    sw: merged.sw,
                    cap: merged.cap.clone(),
                    join: merged.join.clone(),
                });
                false
            }
            "circle" => {
                shapes.push(RawShape {
                    d: circle_d(
                        num(&attrs, "cx", 0.0),
                        num(&attrs, "cy", 0.0),
                        num(&attrs, "r", 0.0),
                    ),
                    fill: merged.fill.clone(),
                    stroke: merged.stroke.clone(),
                    sw: merged.sw,
                    cap: merged.cap.clone(),
                    join: merged.join.clone(),
                });
                false
            }
            "polygon" => {
                println!("vector: unsupported element <polygon>");
                false
            }
            _ => true,
        };
        if _self_closing {
        } else if group {
            open.push((name, true));
            ctx.push(merged);
        } else {
            open.push((name, false));
        }
    }
    Some((units, shapes))
}

fn rrect_d(x: f64, y: f64, w: f64, h: f64, r: f64) -> String {
    let r = r.min(w / 2.0).min(h / 2.0);
    let k = 0.5523 * r;
    let (x2, y2) = (x + w, y + h);
    format!(
        "M{} {}L{} {}C{} {} {} {} {} {}L{} {}C{} {} {} {} {} {}L{} {}C{} {} {} {} {} {}L{} {}C{} {} {} {} {} {}Z",
        x + r,
        y,
        x2 - r,
        y,
        x2 - r + k,
        y,
        x2,
        y + r - k,
        x2,
        y + r,
        x2,
        y2 - r,
        x2,
        y2 - r + k,
        x2 - r + k,
        y2,
        x2 - r,
        y2,
        x + r,
        y2,
        x + r - k,
        y2,
        x,
        y2 - r + k,
        x,
        y2 - r,
        x,
        y + r,
        x,
        y + r - k,
        x + r - k,
        y,
        x + r,
        y,
    )
}

fn circle_d(cx: f64, cy: f64, r: f64) -> String {
    let k = 0.5523;
    format!(
        "M{} {}C{} {} {} {} {} {}C{} {} {} {} {} {}C{} {} {} {} {} {}C{} {} {} {} {} {}Z",
        cx,
        cy - r,
        cx + k * r,
        cy - r,
        cx + r,
        cy - k * r,
        cx + r,
        cy,
        cx + r,
        cy + k * r,
        cx + k * r,
        cy + r,
        cx,
        cy + r,
        cx - k * r,
        cy + r,
        cx - r,
        cy + k * r,
        cx - r,
        cy,
        cx - r,
        cy - k * r,
        cx - k * r,
        cy - r,
        cx,
        cy - r,
    )
}

fn rgba_of(s: Option<&String>, name: &str) -> Option<gdk::RGBA> {
    let raw = s.filter(|x| !x.is_empty() && *x != "none")?;
    let c = match gdk::RGBA::parse(raw) {
        Ok(c) => c,
        Err(_) => {
            println!("vector: {name}: unparsable color {raw:?}");
            return None;
        }
    };
    Some(c)
}

fn cap_of(s: &str) -> gsk::LineCap {
    match s {
        "round" => gsk::LineCap::Round,
        "square" => gsk::LineCap::Square,
        _ => gsk::LineCap::Butt,
    }
}
fn join_of(s: &str) -> gsk::LineJoin {
    match s {
        "round" => gsk::LineJoin::Round,
        "bevel" => gsk::LineJoin::Bevel,
        _ => gsk::LineJoin::Miter,
    }
}

fn build(name: &str, raw: &[RawShape]) -> Vec<Shape> {
    let mut out = Vec::new();
    for rs in raw {
        let path = match gsk::Path::parse(&rs.d) {
            Ok(p) => p,
            Err(_) => {
                println!("vector: {name}: unparsable path {:.40}…", rs.d);
                continue;
            }
        };
        let f = rgba_of(rs.fill.as_ref(), name);
        let sc = rgba_of(rs.stroke.as_ref(), name);
        if f.is_none() && sc.is_none() {
            continue;
        }
        let stroke = sc.map(|_| {
            let st = gsk::Stroke::new(rs.sw as f32);
            st.set_line_cap(cap_of(&rs.cap));
            st.set_line_join(join_of(&rs.join));
            st
        });
        out.push(Shape {
            path,
            fill: f,
            stroke,
            stroke_color: sc,
        });
    }
    out
}

struct Shape {
    path: gsk::Path,
    fill: Option<gdk::RGBA>,
    stroke: Option<gsk::Stroke>,
    stroke_color: Option<gdk::RGBA>,
}

#[derive(Default)]
pub struct VecIconImp {
    shapes: RefCell<Vec<Shape>>,
    units: Cell<f32>,
}

glib::wrapper! {
    pub struct VecIcon(ObjectSubclass<VecIconImp>) @implements gdk::Paintable;
}

#[glib::object_subclass]
impl ObjectSubclass for VecIconImp {
    const NAME: &'static str = "AlpacaVecIcon";
    type Type = VecIcon;
    type ParentType = glib::Object;
    type Interfaces = (gdk::Paintable,);
}

impl ObjectImpl for VecIconImp {}

impl PaintableImpl for VecIconImp {
    fn snapshot(&self, snapshot: &gdk::Snapshot, width: f64, height: f64) {
        let (w, h) = (width as f32, height as f32);
        let units = self.units.get();
        let side = if w.min(h) > 0.0 && units > 0.0 {
            w.min(h)
        } else {
            0.0
        };
        let s = side / units;
        if s <= 0.0 {
            return;
        }
        let Some(sn) = snapshot.downcast_ref::<gtk4::Snapshot>() else {
            return;
        };
        sn.translate(&graphene::Point::new(
            (w - s * units) / 2.0,
            (h - s * units) / 2.0,
        ));
        sn.scale(s, s);
        for sh in self.shapes.borrow().iter() {
            if let Some(f) = &sh.fill {
                sn.append_fill(&sh.path, gsk::FillRule::Winding, f);
            }
            if let (Some(st), Some(c)) = (&sh.stroke, &sh.stroke_color) {
                sn.append_stroke(&sh.path, st, c);
            }
        }
    }

    fn flags(&self) -> gdk::PaintableFlags {
        gdk::PaintableFlags::STATIC_SIZE | gdk::PaintableFlags::STATIC_CONTENTS
    }
    fn intrinsic_width(&self) -> i32 {
        self.units.get().round() as i32
    }
    fn intrinsic_height(&self) -> i32 {
        self.units.get().round() as i32
    }
    fn intrinsic_aspect_ratio(&self) -> f64 {
        1.0
    }
}

fn load(name: &str) -> Option<(f32, Vec<RawShape>)> {
    let bytes = badges::svg_data(name)?;
    let text = std::str::from_utf8(bytes).ok()?;
    let (units, raw) = walk(text)?;
    if raw.is_empty() {
        println!("vector: {name}: no shapes walked");
        return None;
    }
    Some((units, raw))
}

pub(crate) fn icon(name: &str) -> Option<VecIcon> {
    static CACHE: LazyLock<Mutex<HashMap<String, Option<ParsedIcon>>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    let (units, raw) = CACHE
        .lock()
        .unwrap()
        .entry(name.to_string())
        .or_insert_with(|| load(name))
        .clone()?;
    let shapes = build(name, &raw);
    if shapes.is_empty() {
        println!("vector: {name}: no drawable shapes");
        return None;
    }
    let obj: VecIcon = glib::Object::builder::<VecIcon>().build();
    let imp = obj.imp();
    imp.units.set(units);
    *imp.shapes.borrow_mut() = shapes;
    Some(obj)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::badges::svg_data;

    const GOLDEN: [(&str, f32, usize); 13] = [
        ("bot.svg", 16.0, 5),
        ("branch.svg", 16.0, 4),
        ("chevron-down.svg", 16.0, 1),
        ("chevron.svg", 16.0, 1),
        ("folder.svg", 16.0, 2),
        ("pencil.svg", 16.0, 2),
        ("play.svg", 16.0, 1),
        ("plus.svg", 16.0, 2),
        ("prompt.svg", 16.0, 3),
        ("stop.svg", 16.0, 1),
        ("terminal.svg", 16.0, 2),
        ("trash.svg", 16.0, 5),
        ("x-dim.svg", 16.0, 2),
    ];

    #[test]
    fn all_icons_parse_to_expected_shape_counts() {
        for (name, units, count) in GOLDEN {
            let bytes = svg_data(name).unwrap_or_else(|| panic!("{name}: no svg data"));
            let text = std::str::from_utf8(bytes).unwrap();
            let (u, raw) = walk(text).unwrap_or_else(|| panic!("{name}: walk failed"));
            assert!((u - units).abs() < 1e-4, "{name}: units {u}");
            assert_eq!(raw.len(), count, "{name}: raw shape count");
        }
    }

    #[test]
    fn vecicon_snapshot_records_nodes() {
        gtk4::init().unwrap();
        for (name, _, _) in GOLDEN {
            let v = icon(name).unwrap_or_else(|| panic!("{name}: icon None"));
            let snap = gtk4::Snapshot::new();
            v.snapshot(&snap, 16.0, 16.0);
            assert!(snap.to_node().is_some(), "{name}: no nodes recorded");
        }
    }

    #[test]
    fn missing_icon_returns_none() {
        assert!(icon("nope.svg").is_none());
    }
}
