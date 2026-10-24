mod fs;
mod git;
mod render;
mod support;
mod ui;

fn main() {
    ui::app::run(std::env::args().nth(1));
}
