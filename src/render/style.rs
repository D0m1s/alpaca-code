use std::fs;
use std::path::{Path, PathBuf};

use gtk4::prelude::*;

const CSS: &str = include_str!("../../assets/app.css");
const SCHEME: &str = include_str!("../../assets/style-schemes/alpaca-dark.xml");
const ICON_THEME_BUNDLE: &[u8] = include_bytes!("../../assets/app-icons.gresource");
const ICON_SIZES: [(&str, &[u8]); 6] = [
    (
        "16x16",
        include_bytes!("../../assets/icons/io.alpaca.rs-16.png"),
    ),
    (
        "32x32",
        include_bytes!("../../assets/icons/io.alpaca.rs-32.png"),
    ),
    (
        "48x48",
        include_bytes!("../../assets/icons/io.alpaca.rs-48.png"),
    ),
    (
        "64x64",
        include_bytes!("../../assets/icons/io.alpaca.rs-64.png"),
    ),
    (
        "128x128",
        include_bytes!("../../assets/icons/io.alpaca.rs-128.png"),
    ),
    (
        "256x256",
        include_bytes!("../../assets/icons/io.alpaca.rs-256.png"),
    ),
];

fn data_home() -> PathBuf {
    std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").expect("HOME unset")).join(".local/share")
        })
}

fn dirs_xdg() -> PathBuf {
    data_home().join("alpaca-code-rs")
}

fn scheme_dir() -> PathBuf {
    let dir = dirs_xdg().join("style-schemes");
    fs::create_dir_all(&dir).expect("create style-scheme dir");
    let target = dir.join("alpaca-dark.xml");
    if fs::read_to_string(&target).ok().as_deref() != Some(SCHEME) {
        fs::write(&target, SCHEME).expect("write scheme");
    }
    dir
}

pub fn write_app_icons(data_root: &Path) {
    for (size, bytes) in &ICON_SIZES {
        let dir = data_root.join("icons/hicolor").join(*size).join("apps");
        fs::create_dir_all(&dir).expect("create hicolor icon dir");
        let target = dir.join("io.alpaca.rs.png");
        if fs::read(&target).ok().as_deref() != Some(*bytes) {
            fs::write(&target, bytes).expect("write app icon");
        }
    }
}

pub fn init_icons() {
    write_app_icons(&data_home());

    let bundle = gtk4::gio::Resource::from_data(&gtk4::glib::Bytes::from_static(ICON_THEME_BUNDLE))
        .expect("app-icons.gresource is a valid bundle");
    gtk4::gio::resources_register(&bundle);
    gtk4::IconTheme::for_display(&gtk4::gdk::Display::default().unwrap())
        .add_resource_path("/io/alpaca/theme");
}

pub fn init() {
    let settings = gtk4::Settings::default().unwrap();
    settings.set_property("gtk-application-prefer-dark-theme", true);
    settings.set_property("gtk-decoration-layout", ":minimize,maximize,close");
    sourceview5::StyleSchemeManager::default().append_search_path(scheme_dir().to_str().unwrap());
    init_icons();
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(CSS);
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().unwrap(),
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_USER,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_icons_install_completely_and_drift() {
        let root = std::env::temp_dir().join(format!("alpaca-icons-{}", std::process::id()));
        write_app_icons(&root);
        for (size, bytes) in &ICON_SIZES {
            let target = root
                .join("icons/hicolor")
                .join(size)
                .join("apps/io.alpaca.rs.png");
            assert_eq!(fs::read(&target).unwrap().as_slice(), *bytes);
        }
        std::fs::remove_dir_all(&root).ok();
    }
}
