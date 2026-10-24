use std::fs;
use std::path::PathBuf;

use gtk4::prelude::*;

const CSS: &str = include_str!("../../assets/app.css");
const SCHEME: &str = include_str!("../../assets/style-schemes/alpaca-dark.xml");

fn dirs_xdg() -> PathBuf {
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").expect("HOME unset")).join(".local/share"));
    base.join("alpaca-code-rs")
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

pub fn init() {
    let settings = gtk4::Settings::default().unwrap();
    settings.set_property("gtk-application-prefer-dark-theme", true);
            settings.set_property("gtk-decoration-layout", ":minimize,maximize,close");
    sourceview5::StyleSchemeManager::default()
        .append_search_path(scheme_dir().to_str().unwrap());
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(CSS);
        gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().unwrap(),
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_USER,
    );
}