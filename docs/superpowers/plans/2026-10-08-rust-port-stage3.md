# Rust Port Stage 3 (Pixel Polish) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the spec's S3 row — `_TabDistributor` equal-share tab allocation, VecIcon GSK paintables for widget icons, and a python-rust parity sweep — so the rust app's pixels match the python reference on the measured probe rig.

**Architecture:** No new dependency (spec §5): a minimal SVG walker (art-only element subset, `gsk::Path::parse` does the d-strings) feeds a `gdk::Paintable` subclass that replaces the deprecated raster widget-icon path; `editor.rs` gains the vertical `tabs_wrap` Box + the `_TabDistributor` layout-manager port behind the notebook. Verification extends the S2 env-gated probe pattern (`ALPACA_PROBE_S3` + bash wrapper) to resize laws, then one KWin/spectacle sweep compares python vs rust side by side.

**Tech Stack:** relm4 0.11 / gtk4 0.11 (gsk via gtk4 re-exports, subclass traits), gdk4 `PaintableImpl`, no XML crate, no new deps.

**Spec:** `docs/superpowers/specs/2026-10-07-rust-port-design.md` (§4 module map: vector.py→vector.rs is S3; §5 assets: VecIcon replaces widget icons, tree cells stay pixbuf; §9 stage table + acceptance; §10 non-goals: parity only, no ListView, no packaging)

## Global Constraints

- **No git commits** — standing waiver ("Looks good, ignore commit steps"). Ledger lines are the task-complete records; the workspace directory is kept.
- Python stays the UI reference; python code untouched. The ONLY python-repo file edits allowed are CLAUDE.md doc rows (T4, last step).
- Test command: `cargo test --manifest-path alpaca-code-rs/Cargo.toml` (displayless). A runnable bin requires `cargo build` after the final test green — `cargo test` does NOT refresh `target/debug/alpaca-code-rs`.
- Probe discipline (spec §6): rust-widget numbers are re-measured live on the probe rig, never inherited from python-era values (CLAUDE.md's CHROME 91 is stale; the code value 62 is the starting hypothesis — re-measure, don't inherit).
- CSS/paint changes need a REAL restart: kill the exact old pid (never `pkill -f` — self-match law), then relaunch; a second spawn merely forwards to the running primary.
- Art are frozen 16-unit, 1:1-authored SVGs (strokes w2 on integer centerlines). Tree cells (filetree/gitview) and editor tab badges STAY pixbuf/Texture by the S2-verified parity ruling — VecIcon is for WIDGET icons only.
- `vector::icon()` returning None (missing/broken file) never panics a widget build: every call site keeps its existing fallback shape.
- Icon pixel semantics per python: run/stop cells `set_pixel_size(14)`; pane-tab, close ✕, branch-menu icons draw intrinsic 16, no pixel_size.
- Python `state.py` ignores `XDG_CONFIG_HOME` (expanduser only) — any step that LAUNCHES python does not rely on an XDG sandbox for it: it backs up the real `~/.config/alpaca-code/state.json` before and restores it after.
- No programmatic TOPLEVEL resize in probes (GTK4 void); child positions via `set_position` on panes are legal (S2-measured) — the S3 probe uses exactly that.
- Ported code reads like the surrounding rust: module docs cite the python lines / functions they port.

## Review Focus

Input classes the spec implies but no task's happy-path test exercises — each pinned to its owning task.

1. **Narrow window, 6 open tabs** — the GTK-4.22 trap this stage exists for: tabs must shrink PROPORTIONALLY, never pin at the 72px floor, never read WIDER as the window narrows. Pinned by T3's wrapper asserts (`narrow[i] ≤ wide[i] ∀i`; `max(narrow) < max(wide)`; `max(wide) ≤ share`).
2. **More tabs than room** — all-tab strip overflow into scroll mode: sane allocations with `req` below label naturals (no zero-alloc thrash, no crash). Not a python-probe case (python ran 6 tabs; mirror that); T4's sweep opens extra tabs on the narrow run and reviews the `widths=` profile for thrash.
3. **Missing/broken icon asset at runtime** — every call site stays functional. Pinned by T1's `missing_icon_returns_none` test; T4's sweep reads every call site's fallback arm.
4. **Icon sharpness at HiDPI displays** — the reason VecIcon exists (double-resample blur, measured 2026-10-04). T2's desktop pass verifies px-true ink at this box's actual scale; T4's sweep records the device scale beside the numbers.
5. **Dialog styling parity** — discard/error dialogs use the same deprecated gtk4 APIs python uses; the expected sweep ruling is KEEP (deprecated ≠ divergent). Pinned by T4's dialog step (open a binary file for the error dialog; dirty-tab switch for the discard dialog).

---

### Task 1: `vector.rs` — SVG walker + VecIcon paintable + cached `icon()`

**Files:**
- Create: `alpaca-code-rs/src/vector.rs`
- Modify: `alpaca-code-rs/src/main.rs` (add `mod vector;` to the alphabetical module list)
- Modify: `alpaca-code-rs/src/badges.rs` (make `svg_data` `pub(crate)`)

**Interfaces:**
- Consumes: `badges::svg_data(name: &str) -> Option<&'static [u8]>` — the include_bytes table, the art single source of truth shared with chip rendering (make it `pub(crate)`).
- Produces: `vector::icon(name: &str) -> Option<VecIcon>`, where `VecIcon` is a `glib::Object` implementing `gdk::Paintable` (intrinsic width/height = round(viewBox units), aspect ratio 1.0). Also `pub(crate) walk(text: &str) -> Option<(f32, Vec<RawShape>)>` for the test. T2 consumes `vector::icon` only.

- [ ] **Step 1: Write the failing tests**

Create `alpaca-code-rs/src/vector.rs` as the EXACT file below (final test code; Step 3 inserts the implementation between the doc comment and this test mod):

```rust
//! vector.rs — vector.py port (S3): `data/icons/*.svg` parsed into Gsk paths,
//! drawn as render nodes through a Gdk.Paintable subclass. One parse per file
//! (frozen art, single source of truth); intrinsic size = round(viewBox units)
//! so GtkImage allocates the icon 1:1 to the display grid, and GSK tessellates
//! per frame in device pixels — no pre-rasterized pixbuf in the widget loop
//! (the double resample that blurred the old pipeline, CLAUDE.md 2026-10-04).
//! Tree cells and editor tab badges KEEP pixbuf/Texture (python parity).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::badges::svg_data;
    use gtk4::prelude::*;

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
```

Step 2 verifies RED on missing `walk`/`icon`/`RawShape`; the snapshot test additionally needs `gdk::PaintableExt::snapshot` via the gtk4 prelude (already imported in the test mod above).

- [ ] **Step 2: Verify RED**

Add `mod vector;` to main.rs's alphabetical list. Run:

```
cargo test --manifest-path alpaca-code-rs/Cargo.toml vector
```

Expected: compile failure — `walk`, `icon`, `RawShape` etc. missing in `crate::vector`. That compile error is the RED witness (no implementation exists yet); record the exact message in the ledger. A compile PASS here means Step 1's file was never created — stop and re-check.

- [ ] **Step 3: Implement the module** — port of `alpaca_code/vector.py`, python lines cited inline. Full file content below the test mod:

```rust
use gtk4::gdk;
use gtk4::prelude::*;
use gtk4::{glib, gsk, graphene};
use glib::subclass::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// One drawable shape after the inherited-attrs walk (python `_walk` tuple:
/// "paint d, fill, stroke, width, cap, join").
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
            cap: get("stroke-linecap").or_else(|| top.cap.clone()),
            join: get("stroke-linejoin").or_else(|| top.join.clone()),
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
```

Shape-string ports — EXACT transcription of `_rrect_d`/`_circle_d` (rounded-rect r = min(r, w/2, h/2), kappa 0.5523, four cubics = 34 path numbers in `rrect_d`, 26 in `circle_d`):

```rust
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
```

Color and paint parsing:

```rust
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
        let stroke = sc.map(|c| {
            let mut st = gsk::Stroke::new(rs.sw as f32);
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
```

The paintable subclass:

```rust
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
        // python: Gdk.PaintableFlags.SIZE | CONTENTS — introspection strips
        // the C "STATIC_" prefix; the rust enum name keeps it
        gdk::PaintableFlags::STATIC_SIZE | gdk::PaintableFlags::STATIC_CONTENTS
    }
    fn intrinsic_width(&self) -> i32 { self.units.get().round() as i32 }
    fn intrinsic_height(&self) -> i32 { self.units.get().round() as i32 }
    fn intrinsic_aspect_ratio(&self) -> f64 { 1.0 }
}

fn load(name: &str) -> Option<VecIcon> {
    let bytes = badges::svg_data(name)?;
    let text = std::str::from_utf8(bytes).ok()?;
    let (units, raw) = walk(text)?;
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

/// Cached VecIcon for data/icons/name — None on missing/broken (call sites
/// keep symbolic fallbacks; python parity).
pub(crate) fn icon(name: &str) -> Option<VecIcon> {
    static CACHE: LazyLock<Mutex<HashMap<String, Option<VecIcon>>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    let mut cache = CACHE.lock().unwrap();
    cache
        .entry(name.to_string())
        .or_insert_with(|| load(name))
        .clone()
}
```

(`Object::builder::<VecIcon>()` verified: glib 0.22 `builder<'a, O: IsA<Object> + IsClass>()`, and `glib::wrapper!` auto-emits `IsClass` for wrapper types — so the wrapper is a legal O; zero properties, `.build()`.)

`load` needs `badges::svg_data` — make it `pub(crate) fn svg_data(name: &str) -> Option<&'static [u8]>` in badges.rs (one-word visibility change, nothing else).

- [ ] **Step 4: Run the golden tests — GREEN**

`cargo test --manifest-path alpaca-code-rs/Cargo.toml vector`
Expected: 3/3 pass, output pristine — no stray `vector:` println hits the output unparsable-color or unparsable-path fallthroughs (a print = a parse-fallthrough the art shouldn't trigger; investigate, don't tolerance it).

- [ ] **Step 5: Whole suite + first real use**

`cargo test --manifest-path alpaca-code-rs/Cargo.toml` → all green (was 42, now 45). Then `cargo build --manifest-path alpaca-code-rs/Cargo.toml` (the runnable bin keeps compiling — no call site switched YET; T2 switches).

- [ ] **Step 6: Ledger**

`Task 1: complete — vector.rs (walk/build/VecIcon/icon); golden 12/12; snapshot smoke; missing→None; suite 45/45. RED = compile error on missing module. Ruling: <any port deltas python would tolerate — record e.g. width-attr parse fallback> — <why> — <cost>`. NO commit step (waiver).

---

### Task 2: Widget-icon swap — `vector::icon()` at all 12 widget sites, delete `badges::widget_icon`

**Files:**
- Modify: `alpaca-code-rs/src/app.rs` (run/stop cells, current lines ~143/152)
- Modify: `alpaca-code-rs/src/panes.rs` (pane nb page icons, ~345)
- Modify: `alpaca-code-rs/src/editor.rs` (close ✕ at ~621, ~932)
- Modify: `alpaca-code-rs/src/branchmenu.rs` (4 sites, ~98/131/142/152)
- Modify: `alpaca-code-rs/src/badges.rs` (DELETE `widget_icon` + its `#[allow(...)]`/deprecated allowance)

**Interfaces:**
- Consumes: `vector::icon(name) -> Option<VecIcon>` (Task 1), `gtk::Image::from_paintable(Some(&vec))`.
- Produces: none new — call-site swap only. Postcondition: `grep -rn widget_icon alpaca-code-rs/src/` = empty.

**Step 0 ruling (ledger BEFORE editing):** the 12 changes are visual-pipeline swaps; the displayless suite cannot instantiate GtkImage paint allocations, so a RED for THIS task is unbuildable — the gate is T1's snapshot smoke (shared by all swaps) + Step 4's desktop visual pass. Ledger it:

`Ruling: T2's TDD gate = T1 smoke (paintable records nodes) + desktop probe (Step 4) — a displayless unit RED cannot exist for GtkImage paint behavior; cost if wrong: a broken swap shows as blur/invisible ink, caught by T4's px-true sweep compare.`

- [ ] **Step 1: Swap the sites** — three canonical shapes, one per current-code form:

Shape A — px-sized (run/stop; current code is `badges::widget_icon(name, 14).unwrap_or_else(|| SymbolicFallback)`):

```rust
let run_icon = match vector::icon("play.svg") {
    Some(vec) => {
        let img = gtk::Image::from_paintable(Some(&vec));
        img.set_pixel_size(14);
        img
    }
    None => gtk::Image::builder().icon_name("media-playback-start-symbolic").build(),
};
```

Shape B — set_child-if-let (editor ✕; current is `if let Some(img) = badges::widget_icon("x-dim.svg", 16) { close.set_child(...) } else { symbolic }`):

```rust
if let Some(vec) = vector::icon("x-dim.svg") {
    close.set_child(Some(&gtk::Image::from_paintable(Some(&vec))));
} else {
    close.set_child(Some(
        &gtk::Image::builder().icon_name("window-close-symbolic").build(),
    ));
}
```

Shape C — Option-passed-to-helper (panes' page icons + branchmenu strips): replace `badges::widget_icon(name, 16)` with `vector::icon(name).map(|vec| gtk::Image::from_paintable(Some(&vec)))`, keeping the surrounding if-let/Option flow exactly as-is (branchmenu sites append the image when Some, skip when None; panes passes the Option into `pane_tab` which already None-skips the slot).

Per-site table (icon + any special handling, python laws):

| Site | Icon | Pixel size | Fallback / None arm |
|---|---|---|---|
| app.rs run cell | `play.svg` | 14 | symbolic `media-playback-start` (Shape A) |
| app.rs stop cell | `stop.svg` | 14 | symbolic `media-playback-stop` (Shape A) |
| panes.rs pane pages `bot/prompt/terminal.svg` | per-page | none (16 intrinsic) | None arm → pane_tab's existing None path (Shape C) |
| editor.rs open-file ✕ ~621 | `x-dim.svg` | none | symbolic `window-close` (Shape B) |
| editor.rs diff-page ✕ ~932 | `x-dim.svg` | none | symbolic `window-close` (Shape B) |
| branchmenu.rs pill icon ~98 | `branch.svg` | none | None arm = append-nothing (current shape) |
| branchmenu.rs switch-row strip icon ~131 | `branch.svg` | none | same |
| branchmenu.rs switch-row chevron ~142 | `chevron.svg` | none | same |
| branchmenu.rs new-row plus ~152 | `plus.svg` | none | same |

Keep the existing `set_valign(Center)` lines on run_icon/stop_icon/pane images exactly as they stand (the measured 16.5-center law). `chevron-down.svg` stays unused by call sites — it ships through T1's golden table (art-frozen parity; future branches keep using it via the cache).

- [ ] **Step 2: Prove `widget_icon` dead, then delete it**

`grep -rn "widget_icon" alpaca-code-rs/src/` → the badges.rs definition only (no callers). Then delete `widget_icon` AND its dead-code allowance. Build must compile clean (warnings are bugs, not allowances — a removed fn's allowance dies with it).

- [ ] **Step 3: Suite + build**

```
cargo test --manifest-path alpaca-code-rs/Cargo.toml
cargo build --manifest-path alpaca-code-rs/Cargo.toml
```
Expected: all green, clean warnings.

- [ ] **Step 4: Desktop visual pass** (REAL restart discipline: kill the exact old pid, then relaunch the freshly built bin)

Run the rust app on a real project; spectacle ×2; verify: run/stop ink px-true at this box's screen scale, pane-tab icons 16px no halo, ✕ crisper than the old raster, branch pill/chevron/plus same. Record the device scale beside the numbers for the ledger. If an icon looks soft/doubled: suspect paintable intrinsic vs GtkImage px mismatch, or a leftover raster path (both widgets layered) — fix the site, never the art.

- [ ] **Step 5: Ledger**

`Task 2: complete — 12 sites on vector paintables; widget_icon deleted (grep zero); suite <N>/<N>; desktop pass px-true (scale=<noted>).`

---

### Task 3: `_TabDistributor` port + S3 responsive probe

**Files:**
- Modify: `alpaca-code-rs/src/editor.rs` (tabs_wrap + `TabDistributor` + `EditorMsg::ProbeTrace` arm + `TAB_CHROME`)
- Modify: `alpaca-code-rs/src/app.rs` (S3 probe arm + `probe_s3` field + env check)
- Modify: `alpaca-code-rs/src/gitpanel.rs` (`GitPanelMsg::TreeMin` variant printing the filetree min assert)
- Create (throwaway): `/tmp/probe_s3.sh` — bash wrapper mirroring python `tests/responsive_probe.py` laws; recorded in the ledger after it passes

**Interfaces:**
- Consumes: the editor's existing `nb`, the S2 env-gated probe arm pattern in app.rs, the existing `GitPanelMsg` enum.
- Produces: `TAB_CHROME: i32`, `EditorMsg::ProbeTrace(String)`, `GitPanelMsg::TreeMin`. T4's sweep consumes live app behavior only. (The tabs_wrap Box needs NO surviving handle: python keeps `_tabs_wrap` only as creation glue — editor.py:87-92 — so rust keeps it an init LOCAL; the wrap's lifetime rides on `root.append`, and ProbeTrace reads `nb.allocated_width()`, which equals the wrap width because a vertical Box cross-FILLs the nb.)

- [ ] **Step 1: RED — build the probe first, run it against today's code and watch it fail**

Today's tab widths pin at the 72px floor (no distributor exists): `max(narrow) == max(wide)` → the `max(narrow) < max(wide)` law FAILS. That failing line is the RED.

This step writes the PROBE INFRASTRUCTURE so the binary still compiles WITHOUT the distributor: the app.rs chain, gitpanel's `TreeMin` arm, the editor's `ProbeTrace` arm — plus `TAB_CHROME` and `find_tabname` in editor.rs (both inert with no wrap installed: nothing calls them except `ProbeTrace`). Step 3 then adds ONLY the tabs_wrap + distributor.

`/tmp/probe_s3.sh` (bash):

```bash
#!/bin/bash
# S3 responsive probe (rust side) — mirrors python tests/responsive_probe.py.
# Law: 6 equal-share tabs; hpane 900=wide, 850=narrow;
# narrow[i] <= wide[i]; max(narrow) < max(wide); max(wide) <= share.
set -u
BIN="${1:?usage: probe_s3.sh <path/to/alpaca-code-rs}"
ROOT=/tmp/probe_s3_ws
rm -rf "$ROOT"; mkdir -p "$ROOT/deep/path/segments"
printf 'export default {};\n' > "$ROOT/package-lock.json"
printf 'export const x = 1;\n' > "$ROOT/next.config.ts"
printf 'export const y = 2;\n' > "$ROOT/components.json"
printf 'export const z = 3;\n' > "$ROOT/related-products.tsx"
echo '{}' > "$ROOT/trude.json"
printf 'export const deep = 1;\n' > "$ROOT/deep/path/segments/segment-file-with-long-name.tsx"
XDG_CONFIG_HOME=/tmp/probe_s3_xdg
export XDG_CONFIG_HOME
mkdir -p "$XDG_CONFIG_HOME"
ALPACA_PROBE_S3="$ROOT"
export ALPACA_PROBE_S3
"$BIN" "$ROOT" > /tmp/probe_s3.log 2>&1 &
APID=$!
for i in $(seq 1 300); do grep -q "PROBE S3 DONE" /tmp/probe_s3.log && break; sleep 0.2; done
kill -9 "$APID" 2>/dev/null
wait "$APID" 2>/dev/null

fails=0
while IFS= read -r l; do
  echo "$l"
  echo "$l" | grep -q ': FAIL' && fails=$((fails + 1))
done < <(grep 'PROBE assert:' /tmp/probe_s3.log)

wide_line=$(grep 'PROBE taballoc wide' /tmp/probe_s3.log)
narrow_line=$(grep 'PROBE taballoc narrow' /tmp/probe_s3.log)
if [ -z "$wide_line" ] || [ -z "$narrow_line" ]; then
  echo "taballoc lines missing"
  exit 1
fi
share=$(echo "$wide_line" | sed 's/.*share=\([0-9]*\).*/\1/')
wides=$(echo "$wide_line" | sed 's/.*widths=\[\([^]]*\)\].*/\1/')
narrows=$(echo "$narrow_line" | sed 's/.*widths=\[\([^]]*\)\].*/\1/')
IFS=, read -ra W <<< "$wides"
IFS=, read -ra N <<< "$narrows"
if [ "${#W[@]}" -ne 6 ] || [ "${#N[@]}" -ne 6 ]; then
  echo "expected 6 widths per round; got wide=${#W[@]} narrow=${#N[@]}"
  fails=$((fails + 1))
else
  for i in 0 1 2 3 4 5; do
    if [ "${N[i]}" -gt "${W[i]}" ]; then
      echo "law: narrow[$i] ${N[i]} > wide[$i] ${W[i]}"
      fails=$((fails + 1))
    fi
  done
  nmax=$(printf '%s\n' "${N[@]}" | sort -n | tail -1)
  wmax=$(printf '%s\n' "${W[@]}" | sort -n | tail -1)
  if [ "$nmax" -lt "$wmax" ]; then echo ""; else
    echo "law: max(narrow) $nmax !< max(wide) $wmax"; fails=$((fails + 1)); fi
  if [ "$wmax" -le "$share" ]; then echo "" ; else
    echo "law: max(wide) $wmax > share $share"; fails=$((fails + 1)); fi
fi
if [ "$fails" -gt 0 ]; then echo "s3 probe: $fails fails"; exit 1; fi
echo "s3 probe: all compress"
exit 0
```

(Empty echo branches are just for readability; delete them if the log prints too much.)

app.rs probe arm (mirror S2's shape exactly — the app.rs PROBE-ONLY blocks at lines 296-300/432-445/481-590):

```rust
// PROBE-ONLY (ALPACA_PROBE_S3) — step chain: see steps table on this line.
// 0 = SetWorkspace + 6 OpenFile (batch; widths read ~1.4s later, mapped)
// 1 = static-min asserts (window ≤560 editor ≤300 panes ≤300 + TreeMin ≤170)
// 2 = hpane.set_position(900)
// 3 = editor ProbeTrace("wide")        ← taballoc wide print
// 4 = hpane.set_position(850)
// 5 = editor ProbeTrace("narrow") + "PROBE S3 DONE"
const PROBE_S3_STEP_MS: [u64; 6] = [500, 700, 700, 600, 600, 600];
```

Model field: `probe_s3: Option<(PathBuf, usize)>` beside S2's `probe_ws: Option<(PathBuf, usize)>` (app.rs:92). Enum gains two variants beside S2's pair (`AppMsg::ProbeS2(dir)` / `AppMsg::ProbeS2Step`, app.rs:62-64): `ProbeS3(dir)` + `ProbeS3Step`. In the S2 env-check block (app.rs ~296), add the parallel S3 read — S2's route is env-check → send a message (field set happens in the arm, not init):

```rust
// PROBE-ONLY (ALPACA_PROBE_S3) — never runs in production (env unset)
if let Ok(dir) = std::env::var("ALPACA_PROBE_S3") {
    sender.input(AppMsg::ProbeS3(dir.into()));
}
```

`ProbeS3`'s arm sets the field + schedules step 0, exactly as S2's ProbeS2 arm does (app.rs:433-444 uses `ControlFlow::Break` for one-shot timers — mirror it):

```rust
AppMsg::ProbeS3(dir) => {
    // PROBE-ONLY (ALPACA_PROBE_S3) — never runs in production (env unset);
    // step chain start: schedule step 0 with the table's first gap
    self.probe_s3 = Some((dir, 0));
    let snd = sender.clone();
    glib::timeout_add(std::time::Duration::from_millis(PROBE_S3_STEP_MS[0]), move || {
        snd.input(AppMsg::ProbeS3Step);
        glib::ControlFlow::Break
    });
}
```

`ProbeS3Step`'s arm (the `root` param at app.rs:304 is already the name; the hpane handle comes from the window walk — app.rs:257 sets hpane as the window's DIRECT content child, so `root.first_child().and_downcast::<gtk::Paned>()` reaches it — verified today):

```rust
AppMsg::ProbeS3Step => {
    let Some((ws, step)) = self.probe_s3.as_ref().map(|(w, s)| (w.clone(), *s)) else {
        return;
    };
    println!("PROBE step {}: ts={}", step, epoch_ms());
    let hpane = root.first_child().and_downcast::<gtk::Paned>();
    match step {
        0 => {
            sender.input(AppMsg::SetWorkspace(ws.clone()));
            for f in ["package-lock.json", "next.config.ts", "components.json",
                      "related-products.tsx", "trude.json",
                      "deep/path/segments/segment-file-with-long-name.tsx"] {
                sender.input(AppMsg::OpenFile(ws.join(f)));
            }
        }
        1 => {
            let checks = [
                ("window", root.measure(gtk::Orientation::Horizontal, -1).0, 560),
                ("editor", self.editor.widget().measure(gtk::Orientation::Horizontal, -1).0, 300),
                ("panes", self.panes.widget().measure(gtk::Orientation::Horizontal, -1).0, 300),
            ];
            for (name, got, cap) in checks {
                let ok = got <= cap;
                println!("PROBE assert: s3-min-{name}: {}", if ok { "PASS" } else { "FAIL" });
            }
            self.gitpanel.emit(GitPanelMsg::TreeMin);
        }
        2 => if let Some(h) = &hpane { h.set_position(900); },
        3 => self.editor.emit(EditorMsg::ProbeTrace("wide".into())),
        4 => if let Some(h) = &hpane { h.set_position(850); },
        _ => {
            self.editor.emit(EditorMsg::ProbeTrace("narrow".into()));
            println!("PROBE S3 DONE");
            self.probe_s3 = None;
            return;
        }
    }
    // S2 law carried: driver re-arms by incrementing the pair in place
    if let Some(next) = PROBE_S3_STEP_MS.get(step + 1) {
        self.probe_s3.as_mut().unwrap().1 = step + 1;
        let snd = sender.clone();
        glib::timeout_add(std::time::Duration::from_millis(*next), move || {
            snd.input(AppMsg::ProbeS3Step);
            glib::ControlFlow::Break
        });
    }
}
```

(The `GitPanelMsg` import for the TreeMin call joins app.rs's existing use lines; `EditorMsg` already imported for OpenFile routing.)

`EditorMsg::ProbeTrace(String)` arm in editor.rs:

```rust
EditorMsg::ProbeTrace(tag) => {
    let (mut ws, mut ms) = (Vec::new(), Vec::new());
    let strip = self.nb.allocated_width();
    let act = self.nb.action_widget(gtk::PackType::End).map_or(0, |w| w.allocated_width());
    let n = self.nb.n_pages().max(1) as i32;
    let share = i32::max((strip.saturating_sub(act)) / n - TAB_CHROME, 0);
    for i in 0..self.nb.n_pages() {
        let Some(pg) = self.nb.nth_page(Some(i as i32)) else { continue };
        let Some(label) = self.nb.tab_label(&pg) else { continue };
        let Some(name) = find_tabname(&label) else { continue };
        let (min, _nat, _mb, _nb) = name.measure(gtk::Orientation::Horizontal, -1);
        ws.push(format!("{}", name.allocated_width()));
        ms.push(format!("{min}"));
    }
    println!("PROBE taballoc {tag}: strip={strip} act={act} share={share} widths=[{}] mins=[{}]",
        ws.join(","), ms.join(","));
}
```

(The `mins=` list lands in the ledger for the floor audit, mirroring python's tabname-min ≥60 assert on the SAME numbers.)

`GitPanelMsg::TreeMin` variant in gitpanel.rs + arm (gitpanel's enum list: `GitPanelMsg` for panel messages, `ProbeChild` is the child-forwarding wrapper — TreeMin is a panel-level assert, it belongs in `GitPanelMsg`):

```rust
GitPanelMsg::TreeMin => {
    let min = self.ft.widget().measure(gtk::Orientation::Horizontal, -1).0;
    let ok = min <= 170;
    println!("PROBE assert: s3-min-tree: {}", if ok { "PASS" } else { "FAIL" });
}
```

(`self.ft` is gitpanel's `Controller<FileTree>` field, line 106.)

editor.rs also gains, in THIS step (inert without the wrap — only `ProbeTrace` calls them):

```rust
/// chrome subtracted before sharing a tab's width: badge slot + ✕ + label/
/// head paddings (@13px measured on python; starting hypothesis from python's
/// code value 62 — re-measure live if `max(wide) > share` ever trips)
const TAB_CHROME: i32 = 62;

/// walk a widget tree hunting a descendant carrying the tab-name class
/// (used by both the distributor's tab walk and ProbeTrace)
fn find_tabname(w: &gtk4::Widget) -> Option<gtk4::Widget> {
    if w.has_css_class("alpaca-tabname") {
        return Some(w.clone());
    }
    let mut kids = Vec::new();
    let mut c = w.first_child();
    while let Some(ch) = c {
        kids.push(ch);
        c = ch.next_sibling();
    }
    for k in &kids {
        if let Some(f) = find_tabname(k) {
            return Some(f);
        }
    }
    None
}
```

- [ ] **Step 2: Verify RED**

```
cargo build --manifest-path alpaca-code-rs/Cargo.toml
bash /tmp/probe_s3.sh alpaca-code-rs/target/debug/alpaca-code-rs
```
Expected: the static-min asserts all PASS (they were S1/S2 work — the probe's OTHER job) then the tab-shape law FAILS on `max(narrow) …` (all six widths read 72 both rounds). Record the exact failing line.

- [ ] **Step 3: Implement the distributor + wrap** (editor.rs; python editor.py:12-49, 87-92 — `TAB_CHROME` and `find_tabname` already landed in Step 1):

```rust
/// `_TabDistributor` port (editor.py:12-49): the layout manager of the
/// vertical wrapper Box around the notebook strip. GTK 4.22 pins scrollable
/// notebook tabs at their minimum width even with room to spare, and scroll
/// mode's visible tail grows WIDER as the window narrows — this layout
/// manager instead floors every tab-name label at
/// min(share, natural): req = (wrap_width − end_action_w) // n − CHROME.
/// Recomputes only on (width, n) change.

#[derive(Default)]
pub struct TabDistributorImp {
    last: Cell<Option<(i32, i32)>>, // (wrap width, nb page count) — python's _last
}

glib::wrapper! {
    pub struct TabDistributor(ObjectSubclass<TabDistributorImp>) @extends gtk::BoxLayout, gtk::LayoutManager;
}

#[glib::object_subclass]
impl ObjectSubclass for TabDistributorImp {
    const NAME: &'static str = "AlpacaTabDistributor";
    type Type = TabDistributor;
    type ParentType = gtk::BoxLayout;
}

impl ObjectImpl for TabDistributorImp {}

impl LayoutManagerImpl for TabDistributorImp {
    fn allocate(&self, widget: &gtk::Widget, width: i32, height: i32, baseline: i32) {
        self.parent_allocate(widget, width, height, baseline);
        let Some(nb) = widget.first_child().and_downcast::<gtk::Notebook>() else { return };
        let n = nb.n_pages();
        if n == 0 || self.last.get() == Some((width, n as i32)) { return; }
        // python: act_w = end-action widget's allocated width; never set in
        // this app → no widget → 0. (kept None-safe for parity)
        let act = nb.action_widget(gtk::PackType::End).map_or(0, |w| w.allocated_width());
        let req = i32::max((width.saturating_sub(act)) / n as i32 - TAB_CHROME, 0);
        for i in 0..n as i32 {
            let Some(pg) = nb.nth_page(Some(i)) else { continue };
            let Some(label) = nb.tab_label(&pg) else { continue };
            let Some(name) = find_tabname(&label) else { continue };
            let nat = name.measure(gtk::Orientation::Horizontal, -1).1;
            name.set_size_request(i32::min(req, nat), -1);
        }
        self.last.set(Some((width, n as i32)));
    }
}
```

(`nb.n_pages()` returns `u32` — the manual gtk4-rs extension, verified in notebook.rs:56; casts to `i32` are explicit. `nb.tab_label(&pg)` takes `&impl IsA<Widget>`, `action_widget` returns `Option<Widget>`.)

Editor init replaces `root.append(&nb)` with the wrap (verified against the crate: `Box::builder().orientation(...)` exists; `n_pages() -> u32` is the manual gtk4-rs extension; the setter is `set_layout_manager(&self, layout_manager: Option<impl IsA<LayoutManager>>)` — takes ownership, and `TabDistributor: IsA<LayoutManager>` holds because the wrapper declares `@extends gtk::BoxLayout, gtk::LayoutManager`):

```rust
let dist: TabDistributor = glib::Object::builder::<TabDistributor>().build();
let tabs_wrap = gtk::Box::builder().orientation(gtk::Orientation::Vertical).build();
// python's comment, carried: the wrap must take the full row width — a
// HORIZONTAL Box gives a non-hexpand child only its own minimum (measured
// 900px wrap → 280px nb); VERTICAL makes x the cross axis where children FILL
tabs_wrap.set_layout_manager(Some(dist)); // widget refs the object; local keeps our borrow
tabs_wrap.append(&nb);
root.append(&tabs_wrap);
```

(No Editor struct change — python keeps `_tabs_wrap` only as creation glue; ProbeTrace reads `nb.allocated_width()`, which the cross-FILL wrap makes equal.)

- [ ] **Step 4: Verify GREEN — wrapper + floor audit**

```
cargo build --manifest-path alpaca-code-rs/Cargo.toml
bash /tmp/probe_s3.sh alpaca-code-rs/target/debug/alpaca-code-rs
```
Expected: `s3 probe: all compress`, exit 0. Also eyeball the log's `mins=` audit — python's law tabname min ≥60 holds when mapped (the distributor's floor lands ≥60 at these window widths). **If `max(wide) > share`: TAB_CHROME is measured too small by (max − share) + 1 — re-measure the real chrome from a fresh alloc-vs-req diff and set the const to that (never fudge the law).** Record the live CHROME next to python's 62.

The overflow case (Review Focus 2) stays out of the automated wrapper: python's probe ran exactly 6 tabs, and the laws are mirrored 1:1 — a manual 8-tab narrow-session check goes in T4's sweep instead.

- [ ] **Step 5: Whole suite + build**

`cargo test --manifest-path alpaca-code-rs/Cargo.toml && cargo build --manifest-path alpaca-code-rs/Cargo.toml` → green.

- [ ] **Step 6: Ledger**

`Task 3: complete — tabs_wrap+TabDistributor; CHROME=<live value> (python 62); RED witness: <law fail line>; wrapper PASS (share@900=.. share@850=..); suite <N>/<N>. Ruling: <any> — <why> — <cost>.`

---

### Task 4: Parity sweep (KWin rig) + dead-code sweep + CLAUDE.md rows + close-out

**Files:**
- Create (throwaway, /tmp): `sweep_rig.js` (KWin side-by-side geometry script), `/tmp/sweep_s3.sh`, a PIL metric helper
- Modify: rust sources ONLY where a sweep finding demands a fix (each fix RED→GREEN or ledgered deviation-ruling)
- Modify: `CLAUDE.md` (doc rows — the ONE python-repo file allowed; the port's final step)

**Interfaces:**
- Consumes: completed T1-T3 app; the KWin geometry rig in memory `kwin-geometry-probe-rig.md` (loadScript + Scripting.start + journalctl, no per-script DBus objects; spectacle ×2; PIL scans; idle-gate + uinput/pointer-calibration extensions when needed).
- Produces: measured parity table + ledger rulings; CLAUDE.md S3 rows; the final built bin.

- [ ] **Step 1: Fixture + state backup** (Global Constraints: python can't be XDG-sandboxed)

```bash
cp ~/.config/alpaca-code/state.json /tmp/sweep_s3_state_backup.json
WS=/tmp/sweep_s3_ws; rm -rf "$WS"; mkdir -p "$WS/sub"
(cd "$WS" && git init -q -b main && git config user.email s@s && git config user.name s && \
  echo "const a=1" > a.ts && echo "const b=2" > sub/b.ts && echo c > c.md && \
  git add -A && git commit -q -m base && \
  echo "const a=2" > a.ts && echo "const b=3" > sub/b.ts && echo d > d.rs)
```

- [ ] **Step 2: Side-by-side python + rust on the KWin rig**

```bash
python3 bin/alpaca-code "$WS" &
# ... rust bin "$WS" &
# KWin script (sweep_rig.js via the memory rig): both windows fixed rects
# (python left half, rust right half, same size+scale), idle-gated, then
# spectacle ×2 per window (stale-frame law), PIL per the rig's pipeline.
```
Distinguish the windows by TITLE pixel band + clientList stack order cross-check — title text is identical (same project). Verify per-shot, never trust stacking order alone.

Shared-state ruling for the dual launch (CLAUDE.md contract: "never launch both on the same project concurrently and toggle tabs"): both apps share `state.json`'s last_project write on launch, and the sweep keeps them side-by-side for CAPTURES ONLY — no tab toggling, no workspace switches in either window while the other is open. Idle captures are read-only observers; the real file is backed up and restored whole (Step 1/Step 7).

- [ ] **Step 3: Metrics — PIL column-run scans per the established rig** (record the table in the ledger; tolerance ±1px EXCEPT where the pixel numbers already carry exact measured values):

| # | Metric | How | Python reference law |
|---|---|---|---|
| 1 | run-cell icon centering | header cell scan → ink center vs cell center | exact 16.5-center equality on both builds |
| 2 | pane-tab icon↔text gap | pane-tabs row column runs | spacing 6 + intrinsic 16 px-true |
| 3 | editor tab widths at default | same as T3's numbers, default window | equal-share via distributor |
| 4 | letter chips (gitview) | chip band scan | bowl-band centering, ≤0.15 py deviation |
| 5 | badge column (tree + CHANGES) | xpad/ycalign bands | S2-port values |
| 6 | dialogs | error dialog (open a stray `.png` via filetree), discard dialog (dirty a.ts, switch) | python-identical style → expected KEEP ruling |
| 7 | device scale (display) | KWin journal | recorded beside every px number |

The `tabs-than-room` Review Focus (2) check: on the python side only, open two extra tabs on a narrow window and record the tab profile; on rust do the same — eyeball for thrash rather than python-exactness (beyond python's own probe's scope).

- [ ] **Step 4: Fix or rule on each red in the table** — each confirmed pixel mismatch = a fix on the named rust site (TDD where displayless-testable, else a probe-driven fix); each python-side divergence from CLAUDE.md's measured law = the rust side is RIGHT and the sweep RULINGS the discrepancy (`Ruling: rust matches law CLAUDE.md row <name>, python's shot drifted — law wins — cost: <if a real user-visible divergence remains>`).

- [ ] **Step 5: Dead-code sweep** (S2-sanctioned rules: python dead → rust drops; python reads → rust reads; un-allow the compile to reveal every one)

Sites named in the S2 ledger: `panes.rs` `PaneExited.status` + `Status.text/cls`; editor.rs stale "T7 wires" allowances (95/98/106); `gitstatus.rs` 20/53; `gitview.rs` `files: Vec<String>` (python's `_files` is dead too). For each: remove the allowance, compile, judge — python-parity-dead → DELETE the field/branch (shrink the msg shape if the field was its only payload use); python USES it → rust must read it (a real bug the sweep catches). Each removal keeps the suite green.

- [ ] **Step 6: CLAUDE.md doc rows** (the port's LAST step)

Add/update, inside the existing Rust sections of the user's CLAUDE.md: (1) rust modules list gains `vector.rs` (SVG → Gsk paintable via gsk::Path::parse; mini art-only XML walker; `icon()` cache; widget icons on paintable) and badges.rs's `widget_icon` deletion note; (2) the probe env row `ALPACA_PROBE_S3` + wrapper, beside the existing probe notes; (3) if T3 re-measured TAB_CHROME, replace the stale CHROME(91) mention for the live value; (4) an S3 close-out line (S1/S2/S3 all landed, waiver kept work uncommitted).

- [ ] **Step 7: Restore state + whole suite + final build**

```bash
cp /tmp/sweep_s3_state_backup.json ~/.config/alpaca-code/state.json
cargo test --manifest-path alpaca-code-rs/Cargo.toml
cargo build --manifest-path alpaca-code-rs/Cargo.toml
```

- [ ] **Step 8: Ledger + close-out**

`Task 4: complete — sweep table <N rows, all ±1px or ruled>; dead-code <list>; CLAUDE.md rows; suite <N>/<N>; final bin built.` Add the S3 close-out paragraph (findings, rulings, deferred minors) mirroring S2's format.

## Completion contract (whole plan)

- Suite green at every task boundary; final `cargo build` after the last test run so `alpaca-code-rs/target/debug/alpaca-code-rs` is current.
- Zero commit steps executed (waiver) — the ledger + this plan + the working tree are the only artifacts.
- CLAUDE.md carries the S3 rows before the final handoff message.
- finishing-a-development-branch degenerates under the waiver: no branch to finish; the final message carries rulings + deferred minors like S2's close-out did, then the user reviews the S3 state.