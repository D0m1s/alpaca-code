//! vector.rs — vector.py port (S3): `data/icons/*.svg` parsed into Gsk paths,
//! drawn as render nodes through a Gdk.Paintable subclass. One parse per file
//! (frozen art, single source of truth); intrinsic size = round(viewBox units)
//! so GtkImage allocates the icon 1:1 to the display grid, and GSK tessellates
//! per frame in device pixels — no pre-rasterized pixbuf in the widget loop
//! (the double resample that blurred the old pipeline, CLAUDE.md 2026-10-04).
//! Tree cells and editor tab badges KEEP pixbuf/Texture (python parity).

use crate::badges;
use gtk4::gdk;
use gtk4::prelude::*;
use gtk4::gdk::subclass::prelude::*; // PaintableImpl + Ext (gdk4 subclass mod prelude)
use gtk4::{glib, gsk, graphene};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// One drawable shape after the inherited-attrs walk (python `_walk` tuple:
/// "paint d, fill, stroke, width, cap, join").
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
        Self { fill: None, stroke: None, sw: 1.0, cap: "butt".into(), join: "miter".into() }
    }
}

fn attr(attrs: &[(String, String)], k: &str) -> Option<String> {
    attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
}
fn num(attrs: &[(String, String)], k: &str, d: f64) -> f64 {
    attr(attrs, k).and_then(|v| v.trim().parse().ok()).unwrap_or(d)
}

fn tag_name(s: &str) -> &str {
    let end = s.find(|c: char| !(c.is_ascii_alphanumeric() || c == ':')).unwrap_or(s.len());
    &s[..end]
}

/// Reads `name="value"` pairs starting after the tag name; returns the pairs,
/// whether the tag self-closed (`/>`), and the index past `>`. Attribute values
/// are the quoting char that starts them (art is double-quoted; python's ET
/// accepted both).
fn read_attrs(text: &str, at: usize) -> Option<(Vec<(String, String)>, bool, usize)> {
    let b = text.as_bytes();
    let mut i = at;
    let mut out: Vec<(String, String)> = Vec::new();
    loop {
        if i >= b.len() { return None; }
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
                if i == nstart { return None; } // not a name start — malformed tag
                let name = text[nstart..i].to_string();
                while i < b.len() && (b[i] as char).is_ascii_whitespace() { i += 1; }
                if b.get(i) == Some(&b'=') {
                    i += 1;
                    while i < b.len() && (b[i] as char).is_ascii_whitespace() { i += 1; }
                    let Some(&q) = b.get(i) else { return None };
                    let Some(vstart) = i.checked_add(1) else { return None };
                    let Some(rel) = text[vstart..].find(q as char) else { return None };
                    out.push((name, text[vstart..vstart + rel].to_string()));
                    i = vstart + rel + 1;
                } else {
                    out.push((name, String::new())); // valueless attr, skipped by getters
                }
            }
        }
    }
}

/// svg viewBox units + shapes via an inherited-attrs walk (python `_walk`:
/// per-element overrides of fill/stroke/stroke-width/stroke-linecap/
/// stroke-linejoin, everything below inheriting the merged state).
///
/// Art-only XML subset: svg/g/path/rect/circle. Element semantics match the
/// python walker over ElementTree (`for child in el` reaches defs children
/// too — parity; the art has none). `<polygon>` is python-supported but the
/// art never uses it — unsupported here and SAYS SO (the golden count test
/// fails visibly if art ever uses it).
///
/// Attribute values are double-quoted (frozen-art rule). viewBox read from the
/// root svg only, exactly python's load(): 4 numbers → units = vb[2], else the
/// width attr, else default 16. (python's float() would crash loudly on a
/// "16px" width string; rust degrades to the default — art uses plain numbers.)
pub(crate) fn walk(text: &str) -> Option<(f32, Vec<RawShape>)> {
    let b = text.as_bytes();
    let n = b.len();
    let mut shapes: Vec<RawShape> = Vec::new();
    let mut ctx: Vec<Ctx> = vec![Ctx::default()];
    // (tag, pushed_ctx) — shapes have no children (python recursion scope) but
    // a NON-self-closed `<path></path>` still needs its matching close pop; a
    // bare String stack would pop the enclosing group's ctx on that closer and
    // corrupt sibling inheritance
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
            let Some(p) = text[i..].find("-->") else { return None };
            i += p + 3;
            continue;
        }
        if text[i..].starts_with("<?") {
            let Some(p) = text[i..].find("?>") else { return None };
            i += p + 2;
            continue;
        }
        if text[i..].starts_with("</") {
            let Some(p) = text[i..].find('>') else { return None };
            let name = tag_name(&text[i + 2..i + p]).to_string();
            // pop the matching open; stop at the first name match (art is
            // well-formed so the nearest entry is the right one)
            while let Some((tag, pushed)) = open.pop() {
                if pushed {
                    ctx.pop();
                }
                if tag == name { break; }
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
            sw: get("stroke-width").and_then(|v| v.trim().parse().ok()).unwrap_or(top.sw),
            cap: get("stroke-linecap").unwrap_or_else(|| top.cap.clone()),
            join: get("stroke-linejoin").unwrap_or_else(|| top.join.clone()),
        };
        let group = match name.as_str() {
            "svg" if open.is_empty() => {
                let vb: Vec<f64> = get("viewBox").unwrap_or_default().replace(',', " ")
                    .split(' ').filter(|p| !p.is_empty())
                    .filter_map(|p| p.trim().parse().ok()).collect();
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
                // python's `rx or ry` quirk (rrect_d caller): rx="0" falls
                // through to the ry value
                let rx = get("rx").and_then(|v| v.trim().parse::<f64>().ok())
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
                    d: circle_d(num(&attrs, "cx", 0.0), num(&attrs, "cy", 0.0), num(&attrs, "r", 0.0)),
                    fill: merged.fill.clone(),
                    stroke: merged.stroke.clone(),
                    sw: merged.sw,
                    cap: merged.cap.clone(),
                    join: merged.join.clone(),
                });
                false
            }
            "polygon" => { println!("vector: unsupported element <polygon>"); false }
            _ => true, // g/defs/any group: carries the merged ctx for children
        };
        if _self_closing {
            // shapes already emitted above; a self-closed group has no
            // children to inherit — track nothing (python: leaf element)
        } else if group {
            open.push((name, true));
            ctx.push(merged);
        } else {
            open.push((name, false));
        }
        // python scope law: shape elements never push inheritance — siblings
        // of the same group read the GROUP's ctx, which stays on the stack
    }
    Some((units, shapes))
}

fn rrect_d(x: f64, y: f64, w: f64, h: f64, r: f64) -> String {
    let r = r.min(w / 2.0).min(h / 2.0);
    let k = 0.5523 * r;
    let (x2, y2) = (x + w, y + h);
    // python chain, positional: M{x+r} {y}L{x2-r} {y}C(x2-r+k) {y} (x2) (y+r-k)
    // (x2) {y+r}L{x2} {y2-r}C(x2) (y2-r+k) (x2-r+k) {y2} (x2-r) {y2}L{x+r} {y2}
    // C(x+r-k) {y2} (x) (y2-r+k) (x) {y2-r}L{x} {y+r}C(x) (y+r-k) (x+r-k) {y}
    // (x+r) {y}Z — M2 L2 C6 ×4 = 34 numbers
    format!(
        "M{} {}L{} {}C{} {} {} {} {} {}L{} {}C{} {} {} {} {} {}L{} {}C{} {} {} {} {} {}L{} {}C{} {} {} {} {} {}Z",
        x + r, y,
        x2 - r, y,
        x2 - r + k, y, x2, y + r - k, x2, y + r,
        x2, y2 - r,
        x2, y2 - r + k, x2 - r + k, y2, x2 - r, y2,
        x + r, y2,
        x + r - k, y2, x, y2 - r + k, x, y2 - r,
        x, y + r,
        x, y + r - k, x + r - k, y, x + r, y,
    )
}

fn circle_d(cx: f64, cy: f64, r: f64) -> String {
    let k = 0.5523;
    // M(cx) (cy-r) then four cubics around: right, bottom, left, top
    format!(
        "M{} {}C{} {} {} {} {} {}C{} {} {} {} {} {}C{} {} {} {} {} {}C{} {} {} {} {} {}Z",
        cx, cy - r,
        cx + k * r, cy - r, cx + r, cy - k * r, cx + r, cy,
        cx + r, cy + k * r, cx + k * r, cy + r, cx, cy + r,
        cx - k * r, cy + r, cx - r, cy + k * r, cx - r, cy,
        cx - r, cy - k * r, cx - k * r, cy - r, cx, cy - r,
    )
}

fn rgba_of(s: Option<&String>, name: &str) -> Option<gdk::RGBA> {
    let raw = s.as_deref().filter(|x| !x.is_empty() && *x != "none")?;
    let c = match gdk::RGBA::parse(raw) {
        Ok(c) => c,
        Err(_) => { println!("vector: {name}: unparsable color {raw:?}"); return None; }
    };
    Some(c)
}

fn cap_of(s: &str) -> gsk::LineCap {
    match s { "round" => gsk::LineCap::Round, "square" => gsk::LineCap::Square, _ => gsk::LineCap::Butt }
}
fn join_of(s: &str) -> gsk::LineJoin {
    match s { "round" => gsk::LineJoin::Round, "bevel" => gsk::LineJoin::Bevel, _ => gsk::LineJoin::Miter }
}

/// python load's shape-build loop: parse each d, skip unparsable paths and
/// shapes that paint nothing (fill AND stroke both None).
fn build(name: &str, raw: &[RawShape]) -> Vec<Shape> {
    let mut out = Vec::new();
    for rs in raw {
        let path = match gsk::Path::parse(&rs.d) {
            Ok(p) => p,
            Err(_) => { println!("vector: {name}: unparsable path {:.40}…", rs.d); continue }
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
        out.push(Shape { path, fill: f, stroke, stroke_color: sc });
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
    // runtime GType interface registration — wrapper!'s @implements is
    // compile-time only; without this, PaintableExt::snapshot panics on
    // upcast (measured: glib object.rs:122 is::<T>() assert)
    type Interfaces = (gdk::Paintable,);
}

impl ObjectImpl for VecIconImp {}

impl PaintableImpl for VecIconImp {
    fn snapshot(&self, snapshot: &gdk::Snapshot, width: f64, height: f64) {
        let (w, h) = (width as f32, height as f32);
        let units = self.units.get();
        let side = if w.min(h) > 0.0 && units > 0.0 { w.min(h) } else { 0.0 };
        let s = side / units;
        if s <= 0.0 {
            return;
        }
        // python do_snapshot: aspect-fit — translate so the units box centers
        // in the bounds, then scale to fill the shorter side exactly
        let Some(sn) = snapshot.downcast_ref::<gtk4::Snapshot>() else {
            // GTK snapshots ARE Gsk-recording in practice; a non-gsk snapshot
            // cannot draw fill/stroke nodes anyway — same visual as an empty
            // drawable, no panic (python would AttributeError here)
            return;
        };
        sn.translate(&graphene::Point::new((w - s * units) / 2.0, (h - s * units) / 2.0));
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
        // python: Gdk.PaintableFlags.SIZE | CONTENTS — same two bits; this
        // build's C enum carries only the STATIC_ members (measured:
        // gdkpaintable.h 4.22 — SIZE/CONTENTS no longer exist)
        gdk::PaintableFlags::STATIC_SIZE | gdk::PaintableFlags::STATIC_CONTENTS
    }
    fn intrinsic_width(&self) -> i32 { self.units.get().round() as i32 }
    fn intrinsic_height(&self) -> i32 { self.units.get().round() as i32 }
    fn intrinsic_aspect_ratio(&self) -> f64 { 1.0 }
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

/// Cached one-time SVG walk for data/icons/name — None on missing/broken
/// (call sites keep symbolic fallbacks; python parity). The plan's cache held
/// VecIcon OBJECTS, but glib Object wrappers and gsk::Path are !Send — a
/// static Mutex can't hold either — so the cache holds the parse instead and
/// each call builds a fresh (cheap) VecIcon over the same parsed shapes.
pub(crate) fn icon(name: &str) -> Option<VecIcon> {
    static CACHE: LazyLock<Mutex<HashMap<String, Option<(f32, Vec<RawShape>)>>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    let Some((units, raw)) = CACHE
        .lock()
        .unwrap()
        .entry(name.to_string())
        .or_insert_with(|| load(name))
        .clone()
    else {
        return None;
    };
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
    use crate::badges::svg_data;

    const GOLDEN: [(&str, f32, usize); 12] = [
        // (icon, viewBox units, walked raw shape count) — frozen against the
        // art on 2026-10-08 (per-file: path+rect+circle element counts:
        // bot 4+1+0=5, branch 2+0+2=4, chevron-down 1, chevron 1, folder 2,
        // hash 4, play 1, plus 2, prompt 2+1=3, stop 1, terminal 2, x-dim 2).
        // A count change means art or walker drifted — re-walk, don't edit.
        ("bot.svg", 16.0, 5),
        ("branch.svg", 16.0, 4),
        ("chevron-down.svg", 16.0, 1),
        ("chevron.svg", 16.0, 1),
        ("folder.svg", 16.0, 2),
        ("hash.svg", 16.0, 4),
        ("play.svg", 16.0, 1),
        ("plus.svg", 16.0, 2),
        ("prompt.svg", 16.0, 3),
        ("stop.svg", 16.0, 1),
        ("terminal.svg", 16.0, 2),
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
        // displayless-safe: gtk4::init opens no display until a widget is shown
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