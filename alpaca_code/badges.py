# Icon pipeline split in two:
# - widgets (Gtk.Image): `icon()` → vector.VecIcon paintable — SVG parsed once
#   into Gsk paths, drawn from render nodes per frame. No pixbuf, sharp at any
#   display scale (the old raster path blurred: loader resample + GtkImage 16px
#   floor upscale).
# - tree cells (GtkCellRendererPixbuf has no paintable property) + cairo text
#   chips: still pixbufs at their exact display size — 1:1 crisp at scale 1.
import os
import alpaca_code.gi_env as ge
ge.require("GdkPixbuf", ("2.0",))
ge.require("GLib", ("2.0",))
from gi.repository import GdkPixbuf, GLib

from . import vector

try:
    import cairo
except ImportError:  # ponytail: svg assets still load; cairo is a PyGObject-adjacent system dep
    cairo = None

SIZE = 16
ICONS = os.path.join(os.path.dirname(__file__), "data", "icons")

def icon(name: str) -> "vector.VecIcon | None":
    """Widget icon as a Gdk.Paintable; None when missing (call sites fall back)."""
    return vector.icon(name)

# ext → (label, label color, chip color or None). chip None = bare colored glyph.
EXT_BADGE = {
    "ts": ("TS", "#ffffff", "#2f80ed"), "tsx": ("TS", "#ffffff", "#2f80ed"),
    "d.ts": ("TS", "#ffffff", "#2f80ed"),
    "js": ("JS", "#f2c94c", None), "jsx": ("JS", "#f2c94c", None),
    "mjs": ("JS", "#f2c94c", None), "cjs": ("JS", "#f2c94c", None),
    "css": ("#", "#4d8ef0", None), "scss": ("#", "#4d8ef0", None), "less": ("#", "#4d8ef0", None),
    "json": ("{ }", "#f2c94c", None),
    "md": ("MD", "#2f80ed", None), "markdown": ("MD", "#2f80ed", None),
    "py": ("py", "#4d8ef0", None), "pyi": ("py", "#4d8ef0", None),
    "html": ("<>", "#e06c75", None), "xml": ("<>", "#4ec9b0", None),
    "yml": ("Y", "#f2c94c", None), "yaml": ("Y", "#f2c94c", None),
    "txt": ("≡", "#8a93a6", None),
}

def for_file(name: str, is_dir: bool = False) -> tuple[str, str, str | None] | None:
    """(label, color, chip-or-None) for the file's extension, None for dirs/unknowns."""
    if is_dir:
        return None
    return EXT_BADGE.get(os.path.splitext(name)[1].lower().lstrip("."))

_SVGS: dict[tuple[str, int], GdkPixbuf.Pixbuf | None] = {}

def svg_icon(name: str, size: int) -> GdkPixbuf.Pixbuf | None:
    """Design SVG from data/icons rasterized at `size`; None when missing.
    Fill colors are baked into the files — do not tint these."""
    key = (name, size)
    if key not in _SVGS:
        try:
            p = GdkPixbuf.Pixbuf.new_from_file_at_size(os.path.join(ICONS, name), size, size)
        except (GLib.Error, OSError):
            p = None
        _SVGS[key] = p
    return _SVGS[key]

def pixbuf_for(name: str, is_dir: bool = False) -> GdkPixbuf.Pixbuf | None:
    if is_dir:
        return folder_pixbuf()
    ext = os.path.splitext(name)[1].lower().lstrip(".")
    if ext in ("css", "scss", "less"):
        return svg_icon("hash.svg", SIZE)   # the vector hash beats cairo text "#" at row size
    return _render(for_file(name)) if for_file(name) else None

def folder_pixbuf() -> GdkPixbuf.Pixbuf | None:
    return svg_icon("folder.svg", SIZE)

def chevron_pixbuf(down: bool) -> GdkPixbuf.Pixbuf | None:
    return svg_icon("chevron-down.svg" if down else "chevron.svg", 12)

def blank_pixbuf() -> GdkPixbuf.Pixbuf | None:
    """12×12 fully transparent file-row chevron slot (keeps tree text aligned with dirs)."""
    pix = GdkPixbuf.Pixbuf.new(GdkPixbuf.Colorspace.RGB, True, 8, 12, 12)
    pix.fill(0)                            # new() doesn't promise zeroed memory
    return pix

def letter_pixbuf(letter: str, hexcol: str) -> GdkPixbuf.Pixbuf | None:
    """Status-letter tile for gitview's tree cells (spec: indicators on the
    left, ref-chip look): the colored glyph on a tile of its own hue washed
    ~12% toward the card background — same _render chip slot as EXT_BADGE."""
    return _render((letter, hexcol, _tile_bg(hexcol)))

_CARD_BG = "#0d1017"     # .alpaca-card fill (main.py) — tiles must read ON it

def _tile_bg(hexcol: str) -> str:
    f, b = _rgb(hexcol), _rgb(_CARD_BG)
    return "#%02x%02x%02x" % tuple(round((b[i] * 0.88 + f[i] * 0.12) * 255) for i in range(3))

_PCS: dict[tuple, GdkPixbuf.Pixbuf | None] = {}

def _rgb(hexstr: str) -> tuple[float, float, float]:
    h = hexstr.lstrip("#")
    return (int(h[0:2], 16) / 255, int(h[2:4], 16) / 255, int(h[4:6], 16) / 255)

def _rounded(c: cairo.Context, x: float, y: float, w: float, h: float, r: float) -> None:
    """Rect with corner arcs. Four 90° sweeps — the old per-corner tuple list used
    wrong angle pairs (one spanned 270°) and carved a notch out of filled shapes."""
    c.move_to(x + r, y)
    c.line_to(x + w - r, y)
    c.arc(x + w - r, y + r, r, -1.5707963267948966, 0)
    c.line_to(x + w, y + h - r)
    c.arc(x + w - r, y + h - r, r, 0, 1.5707963267948966)
    c.line_to(x + r, y + h)
    c.arc(x + r, y + h - r, r, 1.5707963267948966, 3.141592653589793)
    c.line_to(x, y + r)
    c.arc(x + r, y + r, r, 3.141592653589793, 4.71238898038469)
    c.close_path()

def _render(spec: tuple) -> GdkPixbuf.Pixbuf | None:
    if spec in _PCS:
        return _PCS[spec]
    if cairo is None:
        return None
    label, fg, chip = spec
    # Draw at the exact display size. A 4× supersample crushed back to 16px merged
    # bold strokes into unreadable mush ("TS" read as "15"); Cairo's own
    # antialiasing at 1:1 keeps the glyphs legible (probe-measured).
    S = SIZE
    surf = cairo.ImageSurface(cairo.FORMAT_ARGB32, S, S)
    c = cairo.Context(surf)
    if chip:
        c.set_source_rgb(*_rgb(chip))
        _rounded(c, 0, 0, S, S, int(S * 0.27))
        c.fill()
    elif label == "folder":    # fallback when folder.svg is missing: two rounded tabs
        c.set_source_rgb(*_rgb(fg))
        _rounded(c, S / 3, S / 8, 2 * S / 3, S / 4, 1.5)
        c.fill()
        _rounded(c, S / 6, S / 4, 2 * S / 3, S / 2, 2)
        c.fill()
    if label and label != "folder":
        c.set_source_rgb(*_rgb(fg))
        c.select_font_face("sans", cairo.FONT_SLANT_NORMAL, cairo.FONT_WEIGHT_BOLD)
        size = 13
        c.set_font_size(size)
        ex = c.text_extents(label)
        while size > 9 and ex[2] > S - 4:
            size -= 1
            c.set_font_size(size)
            ex = c.text_extents(label)
        tw, th = ex[2], ex[3]
        c.move_to((S - tw) / 2 - ex[0], (S - th) / 2 - ex[1])   # ink-box centering
        c.show_text(label)
    pix = _pixbuf_from_surface(surf)
    _PCS[spec] = pix
    return pix

def _pixbuf_from_surface(surf: cairo.ImageSurface) -> GdkPixbuf.Pixbuf:
    """Unpremultiply cairo ARGB32 (BGRA bytes, little-endian) → straight RGBA GdkPixbuf."""
    w, h, stride = surf.get_width(), surf.get_height(), surf.get_stride()
    data = memoryview(surf.get_data())
    out = bytearray(4 * w * h)
    for row in range(h):
        src = data[row * stride: row * stride + w * 4]
        dst = row * w * 4
        for i in range(w):
            b, g, r, a = src[4 * i: 4 * i + 4]
            if a == 255:
                pass
            elif a:                     # premultiplied → straight, clamped
                r = min(r * 255 // a, 255); g = min(g * 255 // a, 255); b = min(b * 255 // a, 255)
            out[dst + 4 * i + 0] = r
            out[dst + 4 * i + 1] = g
            out[dst + 4 * i + 2] = b
            out[dst + 4 * i + 3] = a
    pix = GdkPixbuf.Pixbuf.new_from_data(bytes(out), GdkPixbuf.Colorspace.RGB, True, 8, w, h, 4 * w, None, None)
    return pix