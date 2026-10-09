//! Icon pipeline: pixbuf surface for tree cells + cairo text chips (python
//! `alpaca_code/badges.py` minus `icon()` — vector widgets are the S3 port).
//! WIDGET icons live in vector.rs (VecIcon paintables, S3); TREE-CELL /
//! tab-badge pixbufs are still rasterized here — CellRendererPixbuf has no
//! paintable property (CLAUDE.md).

use gdk_pixbuf::prelude::*;
use gdk_pixbuf::{Colorspace, Pixbuf, PixbufLoader};
use gtk4::cairo::{Context, FontSlant, Format, FontWeight, ImageSurface};
use std::collections::HashMap;
use std::sync::Mutex;

pub const SIZE: i32 = 16;

/// ext → (label, label color, chip color or None). chip None = bare colored glyph.
const EXT_BADGE: &[(&str, (&str, &str, Option<&str>))] = &[
    ("ts", ("TS", "#ffffff", Some("#2f80ed"))),
    ("tsx", ("TS", "#ffffff", Some("#2f80ed"))),
    ("d.ts", ("TS", "#ffffff", Some("#2f80ed"))),
    ("js", ("JS", "#f2c94c", None)),
    ("jsx", ("JS", "#f2c94c", None)),
    ("mjs", ("JS", "#f2c94c", None)),
    ("cjs", ("JS", "#f2c94c", None)),
    ("css", ("#", "#4d8ef0", None)),
    ("scss", ("#", "#4d8ef0", None)),
    ("less", ("#", "#4d8ef0", None)),
    ("json", ("{ }", "#f2c94c", None)),
    ("md", ("MD", "#2f80ed", None)),
    ("markdown", ("MD", "#2f80ed", None)),
    ("py", ("py", "#4d8ef0", None)),
    ("pyi", ("py", "#4d8ef0", None)),
    ("html", ("<>", "#e06c75", None)),
    ("xml", ("<>", "#4ec9b0", None)),
    ("yml", ("Y", "#f2c94c", None)),
    ("yaml", ("Y", "#f2c94c", None)),
    ("txt", ("≡", "#8a93a6", None)),
];

/// (label, color, chip-or-None) for the file's extension, None for dirs/unknowns.
pub fn for_file(
    name: &str,
    is_dir: bool,
) -> Option<(&'static str, &'static str, Option<&'static str>)> {
    if is_dir {
        return None;
    }
    let ext = ext_of(name);
    EXT_BADGE
        .iter()
        .find(|(e, _)| *e == ext.as_str())
        .map(|(_, spec)| *spec)
}

/// os.path.splitext(name)[1].lower().lstrip(".") — a leading-dot filename
/// (".hidden") counts as extension-less, as does a trailing lone dot.
fn ext_of(name: &str) -> String {
    let base = name.strip_prefix('.').unwrap_or(name);
    base.rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// One rasterization per (name, size), like python's `_SVGS`. Caches the
/// straight-alpha RGBA bytes (Pixbuf itself is !Send).
#[derive(Clone)]
struct PixbufBytes {
    rgba: Vec<u8>,
    w: i32,
    h: i32,
}

static SVG_CACHE: std::sync::LazyLock<
    Mutex<HashMap<(String, i32), Option<PixbufBytes>>>,
> = std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
static CHIP_CACHE: std::sync::LazyLock<Mutex<HashMap<String, Option<PixbufBytes>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

pub(crate) fn svg_data(name: &str) -> Option<&'static [u8]> {
    match name {
        "bot.svg" => Some(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/icons/bot.svg"
        ))),
        "branch.svg" => Some(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/icons/branch.svg"
        ))),
        "chevron-down.svg" => Some(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/icons/chevron-down.svg"
        ))),
        "chevron.svg" => Some(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/icons/chevron.svg"
        ))),
        "folder.svg" => Some(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/icons/folder.svg"
        ))),
        "hash.svg" => Some(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/icons/hash.svg"
        ))),
        "play.svg" => Some(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/icons/play.svg"
        ))),
        "plus.svg" => Some(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/icons/plus.svg"
        ))),
        "prompt.svg" => Some(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/icons/prompt.svg"
        ))),
        "stop.svg" => Some(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/icons/stop.svg"
        ))),
        "terminal.svg" => Some(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/icons/terminal.svg"
        ))),
        "x-dim.svg" => Some(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/icons/x-dim.svg"
        ))),
        _ => None,
    }
}

/// Rasterize an embedded SVG at `size` through the system pixbuf loader.
fn rasterize(name: &str, size: i32) -> Option<PixbufBytes> {
    let loader = PixbufLoader::with_mime_type("image/svg+xml").ok()?;
    loader.set_size(size, size);
    loader.write(svg_data(name)?).ok()?;
    loader.close().ok()?;
    let pix = loader.pixbuf()?;
    Some(PixbufBytes {
        rgba: pix_bytes(&pix),
        w: pix.width(),
        h: pix.height(),
    })
}

/// Straight-alpha RGBA copy of a pixbuf's pixel buffer (read-only FFI slice).
fn pix_bytes(pix: &Pixbuf) -> Vec<u8> {
    use glib::translate::*;
    unsafe {
        let mut len: u32 = 0;
        let p = gdk_pixbuf::ffi::gdk_pixbuf_get_pixels_with_length(pix.to_glib_none().0, &mut len);
        std::slice::from_raw_parts(p, len as usize).to_vec()
    }
}

fn to_pixbuf(b: &PixbufBytes) -> Pixbuf {
    Pixbuf::from_bytes(
        &glib::Bytes::from_owned(b.rgba.clone()),
        Colorspace::Rgb,
        true,
        8,
        b.w,
        b.h,
        b.w * 4,
    )
}

/// Design SVG from the embedded assets rasterized at `size`; None when missing.
/// Fill colors are baked into the files — do not tint these.
pub fn svg_pixbuf(name: &str, size: i32) -> Option<Pixbuf> {
    let cached = SVG_CACHE
        .lock()
        .unwrap()
        .entry((name.to_string(), size))
        .or_insert_with(|| rasterize(name, size))
        .clone();
    cached.as_ref().map(to_pixbuf)
}

pub fn pixbuf_for(name: &str, is_dir: bool) -> Option<Pixbuf> {
    if is_dir {
        return folder_pixbuf();
    }
    let ext = ext_of(name);
    if ["css", "scss", "less"].contains(&ext.as_str()) {
        return svg_pixbuf("hash.svg", SIZE); // the vector hash beats cairo text "#" at row size
    }
    for_file(name, false).and_then(|(l, fg, chip)| render(l, fg, chip))
}

pub fn folder_pixbuf() -> Option<Pixbuf> {
    svg_pixbuf("folder.svg", SIZE)
}

pub fn chevron_pixbuf(down: bool) -> Option<Pixbuf> {
    svg_pixbuf(if down { "chevron-down.svg" } else { "chevron.svg" }, 12)
}

/// 12×12 fully transparent file-row chevron slot (keeps tree text aligned with dirs).
pub fn blank_pixbuf() -> Pixbuf {
    let pix = Pixbuf::new(Colorspace::Rgb, true, 8, 12, 12).unwrap();
    pix.fill(0); // new() doesn't promise zeroed memory
    pix
}

/// gitview status-letter tile (python badges.py letter_tile) — S2 consumer;
/// exercised by the test below until S2 lands.
pub fn letter_pixbuf(letter: &str, hexcol: &str) -> Option<Pixbuf> {
    render(letter, hexcol, Some(&tile_bg(hexcol)))
}

const CARD_BG: &str = "#0d1017"; // .alpaca-card fill — tiles must read ON it

fn rgb(hexstr: &str) -> [f64; 3] {
    let h = hexstr.trim_start_matches('#');
    let chan = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0) as f64 / 255.0;
    [chan(0), chan(2), chan(4)]
}

fn tile_bg(hexcol: &str) -> String {
    let f = rgb(hexcol);
    let b = rgb(CARD_BG);
    let mix = |i: usize| ((b[i] * 0.88 + f[i] * 0.12) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", mix(0), mix(1), mix(2))
}

/// Rect with corner arcs. Four 90° sweeps — the old per-corner tuple list used
/// wrong angle pairs (one spanned 270°) and carved a notch out of filled shapes.
fn rounded(c: &Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    c.move_to(x + r, y);
    c.line_to(x + w - r, y);
    c.arc(x + w - r, y + r, r, -1.5707963267948966, 0.0);
    c.line_to(x + w, y + h - r);
    c.arc(x + w - r, y + h - r, r, 0.0, 1.5707963267948966);
    c.line_to(x + r, y + h);
    c.arc(x + r, y + h - r, r, 1.5707963267948966, 3.141592653589793);
    c.line_to(x, y + r);
    c.arc(x + r, y + r, r, 3.141592653589793, 4.71238898038469);
    c.close_path();
}

fn is_lower(label: &str) -> bool {
    label.chars().any(char::is_lowercase) && !label.chars().any(char::is_uppercase)
}

/// Chip/glyph render at the exact display size (python badges.py:_render).
/// NO supersampling — a 4× supersample crushed back to 16px merged bold
/// strokes into unreadable mush ("TS" read as "15", probe-measured); Cairo's
/// own antialiasing at 1:1 keeps the glyphs legible.
fn render(label: &str, fg: &str, chip: Option<&str>) -> Option<Pixbuf> {
    let key = format!("{label}|{fg}|{chip:?}");
    let cached = CHIP_CACHE
        .lock()
        .unwrap()
        .entry(key)
        .or_insert_with(|| render_bytes(label, fg, chip))
        .clone();
    cached.as_ref().map(to_pixbuf)
}

fn render_bytes(label: &str, fg: &str, chip: Option<&str>) -> Option<PixbufBytes> {
    let s = SIZE as f64;
    let surf = ImageSurface::create(Format::ARgb32, SIZE, SIZE).ok()?;
    let c = Context::new(&surf).ok()?;
    match chip {
        Some(chip) => {
            let rgb = rgb(chip);
            c.set_source_rgb(rgb[0], rgb[1], rgb[2]);
            rounded(&c, 0.0, 0.0, s, s, (s * 0.27).floor());
            c.fill().ok()?;
        }
        // fallback when folder.svg is missing: two rounded tabs
        None if label == "folder" => {
            let rgb = rgb(fg);
            c.set_source_rgb(rgb[0], rgb[1], rgb[2]);
            rounded(&c, s / 3.0, s / 8.0, 2.0 * s / 3.0, s / 4.0, 1.5);
            c.fill().ok()?;
            rounded(&c, s / 6.0, s / 4.0, 2.0 * s / 3.0, s / 2.0, 2.0);
            c.fill().ok()?;
        }
        None => {}
    }
    if !label.is_empty() && label != "folder" {
        let rgb = rgb(fg);
        c.set_source_rgb(rgb[0], rgb[1], rgb[2]);
        c.select_font_face("sans", FontSlant::Normal, FontWeight::Bold);
        let mut size = 13.0;
        c.set_font_size(size);
        let mut ex = c.text_extents(label).ok()?;
        while size > 9.0 && ex.width() > s - 4.0 {
            size -= 1.0;
            c.set_font_size(size);
            ex = c.text_extents(label).ok()?;
        }
        let tw = ex.width();
        if is_lower(label) {
            // lowercase labels: center the BOWL BAND (x-height) at S/2 and let
            // tails hang below — ink-box centering pulls bowls ~1.5px high
            // (descender-tipped box), measured on py chips vs filenames.
            let xh = c.text_extents("x").ok()?.height();
            let base_y = (s / 2.0 + xh / 2.0).round(); // 11 at SIZE 16 — the row's text baseline
            c.move_to((s - tw) / 2.0 - ex.x_bearing(), base_y);
        } else {
            // ink-box centering (caps letters = the band exactly)
            c.move_to(
                (s - tw) / 2.0 - ex.x_bearing(),
                (s - ex.height()) / 2.0 - ex.y_bearing(),
            );
        }
        c.show_text(label).ok()?;
    }
    drop(c); // ImageSurface::data() needs the exclusive refcount the Context holds
    Some(unpremultiply(surf))
}

/// Unpremultiply cairo ARGB32 (BGRA bytes, little-endian) → straight RGBA bytes.
fn unpremultiply(mut surf: ImageSurface) -> PixbufBytes {
    let (w, h) = (surf.width() as usize, surf.height() as usize);
    let stride = surf.stride() as usize;
    let data = surf.data().expect("unpremultiply: cairo surface data");
    let mut out = vec![0u8; 4 * w * h];
    for row in 0..h {
        let src = &data[row * stride..row * stride + w * 4];
        let dst = row * w * 4;
        for i in 0..w {
            let (b, g, r, a) = (src[4 * i], src[4 * i + 1], src[4 * i + 2], src[4 * i + 3]);
            // premultiplied → straight, clamped (python badges.py:174-176)
            let scale = |v: u8| ((v as u32 * 255) / a as u32) as u8;
            let (r, g, b) = match a {
                0 => (0, 0, 0),
                255 => (r, g, b),
                _ => (scale(r), scale(g), scale(b)),
            };
            out[dst + 4 * i] = r;
            out[dst + 4 * i + 1] = g;
            out[dst + 4 * i + 2] = b;
            out[dst + 4 * i + 3] = a;
        }
    }
    PixbufBytes {
        rgba: out,
        w: w as i32,
        h: h as i32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badge_table_and_exts() {
        assert!(for_file("x.ts", false).is_some());
        assert_eq!(for_file("x.ts", false).unwrap().1, "#ffffff"); // label color
        assert!(for_file("x.jsx", false).unwrap().2.is_none()); // bare glyph, no chip
        assert_eq!(for_file("a.js", false).unwrap().0, "JS");
        assert!(for_file("somedir", true).is_none()); // dirs never get chips
        assert!(for_file("noext", false).is_none()); // unknown ext
        // "css" is a NAME not ext — ext-less → None
        assert!(for_file("css", false).is_none());
    }

    #[test]
    fn pixbufs_render_if_art_present() {
        // These render only if the embedded SVG assets exist; they must not panic
        // either way. Assert the sizes when they do load.
        if let Some(p) = folder_pixbuf() {
            assert_eq!(p.width(), 16);
        }
        if let Some(c) = chevron_pixbuf(true) {
            assert_eq!(c.width(), 12);
        }
        let blank = blank_pixbuf();
        assert_eq!((blank.width(), blank.height()), (12, 12));
        // letter tile: 16×16
        if let Some(t) = letter_pixbuf("M", "#f2c94c") {
            assert_eq!(t.width(), 16);
        }
        // the chip path actually paints (blue rounded tile under the glyph)
        let chip = render_bytes("TS", "#ffffff", Some("#2f80ed")).unwrap();
        assert!(
            chip.rgba.chunks_exact(4).any(|px| px[3] > 0),
            "chip render painted nothing"
        );
    }
}