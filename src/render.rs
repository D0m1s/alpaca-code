pub mod badges;
pub mod seti;
pub mod style;
pub mod vector;

#[cfg(test)]
pub(crate) mod tests {
    #[test]
    fn gtk_layer_tests() {
        gtk4::init().unwrap();
        crate::render::style::tests::bundled_app_icon_resolves_by_name();
        crate::render::vector::tests::vecicon_snapshot_records_nodes();
    }
}
