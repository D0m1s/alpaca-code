
use crate::render::seti;
use gdk_pixbuf::prelude::*;
use gdk_pixbuf::{Colorspace, Pixbuf, PixbufLoader};
use gtk4::cairo::{Context, FontSlant, Format, FontWeight, ImageSurface};
use std::collections::HashMap;
use std::sync::Mutex;

pub const SIZE: i32 = 16;

#[derive(Clone)]
struct PixbufBytes {
    rgba: Vec<u8>,
    w: i32,
    h: i32,
}

type BadgeCache = HashMap<(String, i32), Option<PixbufBytes>>;

static SVG_CACHE: std::sync::LazyLock<Mutex<BadgeCache>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
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
        "pencil.svg" => Some(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/icons/pencil.svg"
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
        "trash.svg" => Some(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/icons/trash.svg"
        ))),
        "x-dim.svg" => Some(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/icons/x-dim.svg"
        ))),
        _ => seti::svg_data(name),
    }
}

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

fn pix_bytes(pix: &Pixbuf) -> Vec<u8> {
    use gtk4::glib::translate::*;
    unsafe {
        let mut len: u32 = 0;
        let p = gdk_pixbuf::ffi::gdk_pixbuf_get_pixels_with_length(pix.to_glib_none().0, &mut len);
        std::slice::from_raw_parts(p, len as usize).to_vec()
    }
}

fn to_pixbuf(b: &PixbufBytes) -> Pixbuf {
    Pixbuf::from_bytes(
        &gtk4::glib::Bytes::from_owned(b.rgba.clone()),
        Colorspace::Rgb,
        true,
        8,
        b.w,
        b.h,
        b.w * 4,
    )
}

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
    svg_pixbuf(seti::icon_for(name), SIZE)
}

pub fn folder_pixbuf() -> Option<Pixbuf> {
    svg_pixbuf("folder.svg", SIZE)
}

pub fn chevron_pixbuf(down: bool) -> Option<Pixbuf> {
    svg_pixbuf(if down { "chevron-down.svg" } else { "chevron.svg" }, 12)
}

pub fn blank_pixbuf() -> Pixbuf {
    let pix = Pixbuf::new(Colorspace::Rgb, true, 8, 12, 12).unwrap();
    pix.fill(0); 
    pix
}

pub fn letter_pixbuf(letter: &str, hexcol: &str) -> Option<Pixbuf> {
    render(letter, hexcol, Some(&tile_bg(hexcol)))
}

const CARD_BG: &str = "#0d1017"; 

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

fn rounded(c: &Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    use std::f64::consts::{FRAC_PI_2, PI};
    c.move_to(x + r, y);
    c.line_to(x + w - r, y);
    c.arc(x + w - r, y + r, r, -FRAC_PI_2, 0.0);
    c.line_to(x + w, y + h - r);
    c.arc(x + w - r, y + h - r, r, 0.0, FRAC_PI_2);
    c.line_to(x + r, y + h);
    c.arc(x + r, y + h - r, r, FRAC_PI_2, PI);
    c.line_to(x, y + r);
    c.arc(x + r, y + r, r, PI, PI + FRAC_PI_2); 
    c.close_path();
}

fn is_lower(label: &str) -> bool {
    label.chars().any(char::is_lowercase) && !label.chars().any(char::is_uppercase)
}

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
                                                let xh = c.text_extents("x").ok()?.height();
            let base_y = (s / 2.0 + xh / 2.0).round(); 
            c.move_to((s - tw) / 2.0 - ex.x_bearing(), base_y);
        } else {
                        c.move_to(
                (s - tw) / 2.0 - ex.x_bearing(),
                (s - ex.height()) / 2.0 - ex.y_bearing(),
            );
        }
        c.show_text(label).ok()?;
    }
    drop(c); 
    Some(unpremultiply(surf))
}

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
    fn pixbufs_render_if_art_present() {
                        if let Some(p) = folder_pixbuf() {
            assert_eq!(p.width(), 16);
        }
        if let Some(c) = chevron_pixbuf(true) {
            assert_eq!(c.width(), 12);
        }
        let blank = blank_pixbuf();
        assert_eq!((blank.width(), blank.height()), (12, 12));
                if let Some(t) = letter_pixbuf("M", "#f2c94c") {
            assert_eq!(t.width(), 16);
        }
                let chip = render_bytes("TS", "#ffffff", Some("#2f80ed")).unwrap();
        assert!(
            chip.rgba.as_chunks::<4>().0.iter().any(|px| px[3] > 0),
            "chip render painted nothing"
        );
    }
}