"""Vector icons: `data/icons/*.svg` parsed into Gsk paths, drawn as render nodes.

One parse per file (stdlib ElementTree — art stays the single source of truth,
no XML resample ever). Drawing goes through a Gdk.Paintable: do_snapshot emits
Gsk.FillNode/Gsk.StrokeNode in viewBox units, aspect-fitted to the bounds the
widget gives. GSK tessellates per frame in device pixels, so icons stay sharp
at any display scale with no pre-rasterized pixbuf in the loop — none of the
double-resample (loader downscale + GtkImage 16px floor upscale) that blurred
the old pixbuf pipeline.

Widget side: Gtk.Image.new_from_paintable(vec) — the 16px measure floor equals
the 16-unit intrinsic size, so allocation == drawn == 1:1 design units.
Art rule keeping straight strokes crisp at scale 1 and integer HiDPI scales:
stroke centerlines on the integer grid (2px stroke at x=4 fills exactly px 3,4).
Curves/diagonals anti-alias — inherent, fine (single AA pass, like text).
"""
import os
import xml.etree.ElementTree as ET

import alpaca_code.gi_env as ge
ge.require("Gdk", ("4.0",)); ge.require("Gsk", ("4.0",)); ge.require("GObject", ("2.0",))
ge.require("Graphene", ("1.0",))
from gi.repository import Gdk, Gsk, GObject, Graphene

ICONS = os.path.join(os.path.dirname(__file__), "data", "icons")

_CAP = {"round": Gsk.LineCap.ROUND, "butt": Gsk.LineCap.BUTT, "square": Gsk.LineCap.SQUARE}
_JOIN = {"round": Gsk.LineJoin.ROUND, "bevel": Gsk.LineJoin.BEVEL, "miter": Gsk.LineJoin.MITER}

def _walk(el, fill, stroke, sw, cap, join, out):
    """(paint d, fill, stroke, width, cap, join) per shape, paint attrs inherited."""
    if el.get("fill") is not None:
        fill = el.get("fill")
    if el.get("stroke") is not None:
        stroke = el.get("stroke")
    if el.get("stroke-width") is not None:
        sw = float(el.get("stroke-width"))
    if el.get("stroke-linecap") is not None:
        cap = el.get("stroke-linecap")
    if el.get("stroke-linejoin") is not None:
        join = el.get("stroke-linejoin")
    d = el.get("d")
    if d:
        out.append((d, fill, stroke, sw, cap, join))
    tag = el.tag.rpartition("}")[2]        # svg ns: {http://…svg}rect → rect
    if tag == "rect":
        x, y, w, h = (float(el.get(k, 0)) for k in ("x", "y", "width", "height"))
        rx = el.get("rx") and float(el.get("rx")) or float(el.get("ry", 0))
        out.append((_rrect_d(x, y, w, h, rx), fill, stroke, sw, cap, join))
    elif tag == "circle":
        out.append((_circle_d(float(el.get("cx", 0)), float(el.get("cy", 0)), float(el.get("r", 0))),
                    fill, stroke, sw, cap, join))
    elif tag == "polygon":
        out.append((_polygon_d(el.get("points", "")), fill, stroke, sw, cap, join))
    for child in el:                       # <g> nesting / plain shape elements
        _walk(child, fill, stroke, sw, cap, join, out)

def _rrect_d(x, y, w, h, r) -> str:
    """Rounded rect as path data — 4 cubics, kappa 4·tan(22.5°) ≈ 0.5523."""
    r = min(r, w / 2, h / 2)
    k = 0.5523 * r
    x2, y2 = x + w, y + h
    return (f"M{x + r:g} {y:g}L{x2 - r:g} {y:g}C{x2 - r + k:g} {y:g} {x2:g} {y + r - k:g} {x2:g} {y + r:g}"
            f"L{x2:g} {y2 - r:g}C{x2:g} {y2 - r + k:g} {x2 - r + k:g} {y2:g} {x2 - r:g} {y2:g}"
            f"L{x + r:g} {y2:g}C{x + r - k:g} {y2:g} {x:g} {y2 - r + k:g} {x:g} {y2 - r:g}"
            f"L{x:g} {y + r:g}C{x:g} {y + r - k:g} {x + r - k:g} {y:g} {x + r:g} {y:g}Z")

def _circle_d(cx, cy, r) -> str:
    k = 0.5523
    return (f"M{cx:g} {cy - r:g}"
            f"C{cx + k * r:g} {cy - r:g} {cx + r:g} {cy - k * r:g} {cx + r:g} {cy:g}"
            f"C{cx + r:g} {cy + k * r:g} {cx + k * r:g} {cy + r:g} {cx:g} {cy + r:g}"
            f"C{cx - k * r:g} {cy + r:g} {cx - r:g} {cy + k * r:g} {cx - r:g} {cy:g}"
            f"C{cx - r:g} {cy - k * r:g} {cx - k * r:g} {cy - r:g} {cx:g} {cy - r:g}Z")

def _polygon_d(points: str) -> str:
    pts = points.replace(",", " ").split()
    d = f"M{pts[0]} {pts[1]}"
    for i in range(2, len(pts), 2):
        d += f"L{pts[i]} {pts[i + 1]}"
    return d + "Z"

def _rgba(s: str | None, where: str) -> Gdk.RGBA | None:
    if s in (None, "", "none") or not s:
        return None
    c = Gdk.RGBA()
    if not c.parse(s):
        print(f"vector: {where}: unparsable color {s!r}")
        return None
    return c

def _stroke_of(sw: float, cap: str, join: str) -> Gsk.Stroke:
    st = Gsk.Stroke.new(sw)
    st.set_line_cap(_CAP.get(cap, Gsk.LineCap.BUTT))
    st.set_line_join(_JOIN.get(join, Gsk.LineJoin.MITER))
    return st

class VecIcon(GObject.Object, Gdk.Paintable):
    """One SVG's shapes, immutable; snapshot = viewBox units fit into bounds."""

    __gtype_name__ = "AlpacaVecIcon"

    def __init__(self, name: str, units: float, shapes: list[tuple], **kw):
        super().__init__(**kw)
        self.name = name
        self.units = units
        self._shapes = shapes      # (Gsk.Path, fill RGBA|None, stroke Gsk.Stroke|None, stroke RGBA|None)

    @classmethod
    def load(cls, name: str) -> "VecIcon | None":
        try:
            root = ET.parse(os.path.join(ICONS, name)).getroot()
        except (OSError, ET.ParseError) as e:
            print(f"vector: {name}: load failed ({e})")
            return None
        vb = (root.get("viewBox") or "").replace(",", " ").split()
        units = float(vb[2]) if len(vb) == 4 else float(root.get("width", 16))
        raw: list[tuple] = []
        _walk(root, None, None, 1.0, "butt", "miter", raw)
        shapes = []
        for d, fill, stroke_c, sw, cap, join in raw:
            path = Gsk.Path.parse(d)
            if path is None:
                print(f"vector: {name}: unparsable path {d[:40]!r}")
                continue
            f = _rgba(fill, name)
            s = _rgba(stroke_c, name)
            if f is None and s is None:
                continue           # nothing paints; skip noise
            shapes.append((path, f, _stroke_of(sw, cap, join) if s else None, s))
        if not shapes:
            print(f"vector: {name}: no drawable shapes")
            return None
        return cls(name, units, shapes)

    def do_snapshot(self, snapshot, width, height) -> None:
        side = min(width, height) if min(width, height) and self.units else 0
        s = side / self.units
        if s <= 0:
            return
        snapshot.translate(Graphene.Point().init((width - s * self.units) / 2,
                                                 (height - s * self.units) / 2))
        snapshot.scale(s, s)
        for path, fill, stroke, stroke_c in self._shapes:
            if fill is not None:
                snapshot.append_fill(path, Gsk.FillRule.WINDING, fill)
            if stroke is not None:
                snapshot.append_stroke(path, stroke, stroke_c)

    # Gdk.Paintable: fixed content at fixed size — GTK may cache the recording.
    def do_get_flags(self) -> Gdk.PaintableFlags:
        return Gdk.PaintableFlags.SIZE | Gdk.PaintableFlags.CONTENTS

    def do_get_intrinsic_width(self) -> int:
        return round(self.units)

    def do_get_intrinsic_height(self) -> int:
        return round(self.units)

    def do_get_intrinsic_aspect_ratio(self) -> float:
        return 1.0

_CACHE: dict[str, VecIcon | None] = {}

def icon(name: str) -> VecIcon | None:
    """Cached VecIcon for data/icons/name; None when missing/broken (call sites
    keep their symbolic-icon fallbacks)."""
    if name not in _CACHE:
        _CACHE[name] = VecIcon.load(name)
    return _CACHE[name]

def __main__() -> None:    # displayless smoke: every icon parses and records nodes
    ge.require("Gtk", ("4.0",))
    from gi.repository import Gtk
    snap = Gtk.Snapshot()
    n = 0
    for fn in sorted(os.listdir(ICONS)):
        if not fn.endswith(".svg"):
            continue
        v = icon(fn)
        assert v is not None, fn
        v.do_snapshot(snap, 16, 16)
        n += 1
    assert snap.to_node() is not None
    print(f"vector ok: {n} icons")