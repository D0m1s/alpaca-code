mod app;
mod badges;
mod branchmenu;
mod editor;
mod filetree;
mod gitpanel;
mod gitstatus;
mod gitview;
mod treehover;
mod panes;
mod runctl;
mod state;
mod style;
mod vector;

fn main() {
    // Raw argv handoff: run() resolves the project dir (matches run()'s gate).
    app::run(std::env::args().nth(1));
}