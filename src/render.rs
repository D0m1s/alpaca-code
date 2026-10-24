pub mod badges;
pub mod seti;
pub mod style;
pub mod vector;

#[cfg(test)]
pub(crate) mod tests {
    // Both GTK-touching checks share ONE gtk4::init: the first thread that calls
    // gtk_init becomes GTK's main thread, and libtest runs every #[test] on its
    // own thread — so independent gtk tests cannot coexist in one process
    // (measured 2026-10-25: vecicon snapshot panics with "GTK may only be used
    // from the main thread"). One driver test owns the display for both.
    #[test]
    fn gtk_layer_tests() {
        gtk4::init().unwrap();
        crate::render::style::tests::bundled_app_icon_resolves_by_name();
        crate::render::vector::tests::vecicon_snapshot_records_nodes();
    }
}
