
#![allow(deprecated)]

use std::cell::Cell;
use std::path::{Path, PathBuf};

use relm4::gtk::{self, gdk, gio, glib, pango, prelude::*};
use relm4::component::ComponentController;
use relm4::{Component, ComponentParts, ComponentSender};

use crate::ui::editor::{Editor, EditorMsg, EditorOutput};
use crate::fs::filetree::FileTreeMsg;
use crate::git::gitpanel::{GitPanel, GitPanelMsg, GitPanelOutput, ProbeChild};
use crate::git::gitview::GitViewMsg;
use crate::ui::launcher::{Launcher, LauncherMsg, LauncherOutput};
use crate::ui::panes::{Panes, PanesMsg, PanesOutput, kill_tree};
use crate::support::runctl;
use crate::support::state;
use crate::render::vector;

pub use crate::ui::editor::EditorSnapshot;

#[derive(Debug)]
pub enum AppMsg {
            OpenWorkspace(PathBuf),
        SwitchWorkspace(usize),
        CloseWorkspace(usize),
        AddProjectDialog,
        ShowLauncher,
            LauncherOpen(String),
                LauncherAdd(String),
        OpenNew(PathBuf, String),
            LauncherRename { stem: String, display: String },
            LauncherDelete(String),
        OpenFile(usize, PathBuf),
        EditorState(usize, EditorSnapshot),
        RunStarting(usize),
    RunStatus(usize, bool),
        RunPressed,
    StopPressed,
    FlushTabs,
        SaveWorkbench,
            SaveThenClose,
        SaveAndQuit,
        RecentMissing(String),
        OpenDiff {
        ws: usize,
        rel: String,
        letter: String,
    },
        Commit {
        ws: usize,
        root: PathBuf,
        paths: Vec<String>,
        msg: String,
    },
        Flushed(usize),
        GitChanged(usize, crate::git::gitstatus::Rows),
        WindowActive,
        EditOp(EditKind),
            FocusSearch,
                ProjectCreated { target: PathBuf, res: Result<(), String> },
        ProbeS2(PathBuf),
        ProbeS2Step,
        ProbeS3(PathBuf),
        ProbeS3Step,
            ProbeDraw,
        ProbeS4(PathBuf),
        ProbeS4Step,
            ProbeWBSave(PathBuf),
    ProbeWBStep,
            ProbeWBRestore(PathBuf),
    ProbeWBRStep,
        ProbeLN(PathBuf),
    ProbeLNStep,
}

thread_local! {
        static PROBE_S3_TICKS: Cell<u32> = const { Cell::new(0) };
}

pub const MAX_WORKSPACES: usize = 3;

#[derive(Debug, PartialEq, Eq)]
pub enum AddDecision {
        Switch(usize),
        Add,
        Full,
}

pub fn add_decision(open: &[PathBuf], path: &Path) -> AddDecision {
    if let Some(i) = open.iter().position(|p| p.as_path() == path) {
        return AddDecision::Switch(i);
    }
    if open.len() >= MAX_WORKSPACES {
        return AddDecision::Full;
    }
    AddDecision::Add
}

pub fn close_target(live: &[usize], i: usize) -> Option<usize> {
    live.iter()
        .find(|&&s| s > i)
        .copied()
        .or_else(|| live.iter().rev().find(|&&s| s < i).copied())
}

#[derive(Debug, Clone, Copy)]
pub enum EditKind {
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
}

pub struct Workspace {
    root: PathBuf,
        hpane: gtk::Paned,
            vpane: gtk::Paned,
            btn: gtk::Button,
    editor: relm4::Controller<Editor>,
    panes: relm4::Controller<Panes>,
    gitpanel: relm4::Controller<GitPanel>,
        running: bool,
        snapshot: EditorSnapshot,
            pending_commit: Option<(PathBuf, Vec<String>, String)>,
}

impl Workspace {
                    fn new(path: PathBuf, ws: usize, app_out: ComponentSender<App>, sess: Option<state::ProjectSession>) -> Self {
        let path_str = path.to_string_lossy().into_owned();
        let sess = sess.unwrap_or_default();

                let editor_slot = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let app_in = app_out.clone();
        let editor = Editor::builder()
            .launch(())
            .connect_receiver(move |_, out| match out {
                EditorOutput::StateChanged(s) => app_in.input(AppMsg::EditorState(ws, s)),
                EditorOutput::Flushed => app_in.input(AppMsg::Flushed(ws)), 
            });
        let tabs = state::project_tabs(&state::load(), &path_str);
        editor_slot.append(editor.widget());

                let app_in = app_out.clone();
        let panes = Panes::builder()
            .launch(())
            .connect_receiver(move |_, out| match out {
                PanesOutput::RunStarting => app_in.input(AppMsg::RunStarting(ws)),
                PanesOutput::RunStatus { running } => app_in.input(AppMsg::RunStatus(ws, running)),
            });
        let panes_slot = gtk::Box::new(gtk::Orientation::Vertical, 0);
        panes_slot.append(panes.widget());
        let vpane = gtk::Paned::new(gtk::Orientation::Vertical);
        vpane.set_wide_handle(true);
        vpane.set_start_child(Some(&editor_slot));
        vpane.set_end_child(Some(&panes_slot));
                vpane.set_position(592);
                                if sess.vpane > 0 {
            vpane.set_position(sess.vpane);
        }

                let app_in = app_out.clone();
        let gitpanel = GitPanel::builder()
            .launch(())
            .connect_receiver(move |_, out| match out {
                GitPanelOutput::OpenFile(p) => app_in.input(AppMsg::OpenFile(ws, p)),
                GitPanelOutput::OpenDiff { rel, letter } => {
                    app_in.input(AppMsg::OpenDiff { ws, rel, letter });
                }
                GitPanelOutput::Commit { root, paths, msg } => {
                    app_in.input(AppMsg::Commit { ws, root, paths, msg });
                }
                GitPanelOutput::GitChanged(rows) => app_in.input(AppMsg::GitChanged(ws, rows)),
            });

                        let hpane = gtk::Paned::new(gtk::Orientation::Horizontal);
        hpane.set_wide_handle(true);
        hpane.set_start_child(Some(&vpane));
        hpane.set_end_child(Some(gitpanel.widget()));
                                hpane.set_position(1161);
        if sess.hpane > 0 {
            hpane.set_position(sess.hpane);
        }

                        let btn = gtk::Button::builder()
            .has_frame(false)
            .css_classes(["alpaca-projbtn"])
            .build();
        let lbl = gtk::Label::builder().label(py_basename(&path_str)).build();
        lbl.set_ellipsize(pango::EllipsizeMode::End);
        lbl.set_max_width_chars(24); 
                                                                let close = gtk::Box::builder()
            .css_classes(["alpaca-projclose"])
            .tooltip_text("Close project")
            .build();
                                close.set_valign(gtk::Align::Center);
        if let Some(vec) = vector::icon("x-dim.svg") {
            let img = gtk::Image::from_paintable(Some(&vec));
            img.set_pixel_size(14);
            img.set_valign(gtk::Align::Center);
            close.append(&img);
        } else {
                        close.append(&gtk::Label::builder().label("✕").build());
        }
        let click = gtk::GestureClick::new();
        let snd = app_out.clone();
        click.connect_pressed(move |g, _n, _x, _y| {
                                    if let Some(seq) = g.current_sequence() {
                g.set_sequence_state(&seq, gtk::EventSequenceState::Claimed);
            }
            snd.input(AppMsg::CloseWorkspace(ws));
        });
        close.add_controller(click);
        let pill_child = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        pill_child.append(&lbl);
        pill_child.append(&close);
        btn.set_child(Some(&pill_child));
        btn.set_tooltip_text(Some(&path_str));
                let snd = app_out.clone();
        btn.connect_clicked(move |_b| snd.input(AppMsg::SwitchWorkspace(ws)));

                        gitpanel.emit(GitPanelMsg::SetRoot(path.clone()));
        if !sess.expanded.is_empty() || !sess.changes_closed.is_empty() {
                                                gitpanel.emit(GitPanelMsg::SetTreeState {
                expanded: sess.expanded.clone(),
                closed: sess.changes_closed.clone(),
            });
        }
        panes.emit(PanesMsg::SetRoot(path.clone()));
        editor.emit(EditorMsg::Restore(
            path.clone(),
            tabs.open_tabs.clone(),
            tabs.active_tab,
            sess.scroll.as_ref().map(|s| (s.rel.clone(), s.frac)),
        ));

        Self {
            root: path,
            hpane,
            vpane,
            btn,
            editor,
            panes,
            gitpanel,
            running: false,
            snapshot: EditorSnapshot {
                open_tabs: tabs.open_tabs,
                active_tab: tabs.active_tab,
            },
            pending_commit: None,
        }
    }
}

struct App {
    stack: gtk::Stack,
    proj_box: gtk::Box,
    add_btn: gtk::Button,
        menu_btn: gtk::MenuButton,
    run_btn: gtk::Button,
    stop_btn: gtk::Button,
    run_row: gtk::Box,
                    ws: Vec<Option<Workspace>>,
        active: usize,
            launcher: relm4::Controller<Launcher>,
            probe_ws: Option<(PathBuf, usize)>,
            probe_s3: Option<(PathBuf, usize)>,
            probe_s4: Option<(PathBuf, usize)>,
            probe_wb: Option<(PathBuf, usize)>,
    probe_wbr: Option<(PathBuf, usize)>,
        probe_l: Option<(PathBuf, usize)>,
                wb: Option<state::WorkbenchFile>,
        wb_name: Option<String>,
                wb_display: Option<String>,
}

#[relm4::component]
impl Component for App {
    type CommandOutput = ();
    type Input = AppMsg;
    type Output = ();
    type Init = Option<PathBuf>;

    view! {
        gtk::ApplicationWindow {
            set_title: Some("alpaca-code"),
            set_icon_name: Some("io.alpaca.rs"),
        }
    }

    fn init(model: Self::Init, root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        let arg = model.clone(); 

                                        let run_btn = gtk::Button::new();
        let run_icon = match vector::icon("play.svg") {
            Some(vec) => {
                let img = gtk::Image::from_paintable(Some(&vec));
                img.set_pixel_size(14);
                img
            }
            None => gtk::Image::builder().icon_name("media-playback-start-symbolic").build(),
        };
        run_icon.set_valign(gtk::Align::Center);
        run_btn.set_child(Some(&run_icon));
        run_btn.set_sensitive(false);
        run_btn.set_css_classes(&["alpaca-run"]);
        run_btn.set_tooltip_text(Some("Run"));
        let stop_btn = gtk::Button::new();
        let stop_icon = match vector::icon("stop.svg") {
            Some(vec) => {
                let img = gtk::Image::from_paintable(Some(&vec));
                img.set_pixel_size(14);
                img
            }
            None => gtk::Image::builder().icon_name("media-playback-stop-symbolic").build(),
        };
        stop_icon.set_valign(gtk::Align::Center);
        stop_btn.set_child(Some(&stop_icon));
        stop_btn.set_sensitive(false);
        stop_btn.set_css_classes(&["alpaca-stop"]);
        stop_btn.set_tooltip_text(Some("Stop"));

        let header = gtk::HeaderBar::new();
        header.set_css_classes(&["alpaca-header"]);
                                let proj_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                                                proj_box.set_valign(gtk::Align::Center);
                                                                                let add_btn = gtk::Button::builder()
            .has_frame(false)
            .css_classes(["alpaca-projadd"])
            .build();
        if let Some(vec) = vector::icon("plus.svg") {
            let img = gtk::Image::from_paintable(Some(&vec));
            img.set_pixel_size(14);
            img.set_valign(gtk::Align::Center);
            add_btn.set_child(Some(&img));
        } else {
                        add_btn.set_child(Some(&gtk::Label::builder().label("+").build()));
        }
        add_btn.set_tooltip_text(Some("Open project…"));
        proj_box.append(&add_btn);
        let send = sender.clone();
        add_btn.connect_clicked(move |_b| send.input(AppMsg::AddProjectDialog));
        header.set_title_widget(Some(&proj_box));
                                                        let menu_btn = gtk::MenuButton::new();
        if let Some(vec) = vector::icon("chevron-down.svg") {
            let img = gtk::Image::from_paintable(Some(&vec));
            img.set_pixel_size(14);
            img.set_valign(gtk::Align::Center);
            menu_btn.set_child(Some(&img));
        } else {
                        menu_btn.set_child(Some(&gtk::Image::builder()
                .icon_name("pan-down-symbolic")
                .pixel_size(14)
                .valign(gtk::Align::Center)
                .build()));
        }
        menu_btn.set_tooltip_text(Some("Workbench"));
        menu_btn.set_popover(Some(&gtk::PopoverMenu::from_model(Some(&workbench_menu()))));
        header.pack_start(&menu_btn);

                                        let run_row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        run_row.set_css_classes(&["alpaca-runpill"]);
        for btn in [&run_btn, &stop_btn] {
            let cell = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            cell.set_css_classes(&["alpaca-runcell"]);
            cell.append(btn);
            run_row.append(&cell);
        }
        let divider = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        divider.set_css_classes(&["alpaca-rundivider"]);
        divider.set_margin_top(4);
        divider.set_margin_bottom(4);
        let first = run_row.first_child().unwrap();
        run_row.insert_child_after(&divider, Some(&first)); 
                                        run_row.set_valign(gtk::Align::Center);
        header.pack_end(&run_row);
        root.set_titlebar(Some(&header));

                let send = sender.clone();
        run_btn.connect_clicked(move |_b| send.input(AppMsg::RunPressed));
        let send = sender.clone();
        stop_btn.connect_clicked(move |_b| send.input(AppMsg::StopPressed));

                                let widgets = view_output!();
        let st = state::load();
        let (w, h) = st
            .window
            .map(|g| clamp_to_monitor(g.width, g.height))
            .unwrap_or_else(|| clamp_to_monitor(1586, 992));
        root.set_default_size(w, h);
        if st.window.as_ref().is_some_and(|g| g.maximized) {
            root.maximize();
        }

                        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::None)
            .hexpand(true)
            .vexpand(true)
            .build();
        root.set_child(Some(&stack));

                                let send_l = sender.clone();
        let launcher = Launcher::builder()
            .launch(())
            .connect_receiver(move |_, out| match out {
                LauncherOutput::Open(name) => send_l.input(AppMsg::LauncherOpen(name)),
                LauncherOutput::New(name) => send_l.input(AppMsg::LauncherAdd(name)),
                LauncherOutput::Rename { stem, display } => {
                    send_l.input(AppMsg::LauncherRename { stem, display })
                }
                LauncherOutput::Delete(stem) => send_l.input(AppMsg::LauncherDelete(stem)),
            });
        stack.add_named(launcher.widget(), Some("launcher"));

                                                                        {
            let menu_c = menu_btn.clone();
            let add_c = add_btn.clone();
            stack.connect_notify_local(Some("visible-child"), move |st, _| {
                let on_hub = st.visible_child_name().as_deref() == Some("launcher");
                menu_c.set_visible(!on_hub);
                add_c.set_visible(!on_hub);
            });
        }
        let on_hub = stack.visible_child_name().as_deref() == Some("launcher");
        menu_btn.set_visible(!on_hub);
        add_btn.set_visible(!on_hub);

                let boot_wb = st.last_workbench.as_deref().and_then(state::load_workbench);
    let boot_wb_name = boot_wb.as_ref().map(|w| w.name.clone());
    let model = App {
            stack,
            proj_box,
            add_btn,
            menu_btn,
            run_btn,
            stop_btn,
            run_row,
            launcher,
            ws: Vec::new(),
            active: 0,
            probe_ws: None,
            probe_s3: None,
            probe_s4: None,
            probe_wb: None,
            probe_wbr: None,
            probe_l: None,
                                                            wb: boot_wb,
            wb_name: boot_wb_name,
            wb_display: None,
        };

        register_actions(&relm4::main_application(), &root, sender.clone());

                        let in_active = sender.clone();
        root.connect_notify_local(Some("is-active"), move |_, _| {
            in_active.input(AppMsg::WindowActive)
        });

                                                let snd_close = sender.clone();
        root.connect_close_request(move |_| {
            let _ = snd_close.input_sender().send(AppMsg::SaveThenClose);
            glib::Propagation::Stop
        });

                                if let Some(p) = arg {
            sender.input(AppMsg::OpenWorkspace(p));
        } else {
            sender.input(AppMsg::ShowLauncher);
        }
                if let Ok(dir) = std::env::var("ALPACA_PROBE_S2") {
            sender.input(AppMsg::ProbeS2(PathBuf::from(dir)));
        }
                if let Ok(dir) = std::env::var("ALPACA_PROBE_S3") {
            sender.input(AppMsg::ProbeS3(PathBuf::from(dir)));
        }
                if let Ok(dir) = std::env::var("ALPACA_PROBE_S4") {
            sender.input(AppMsg::ProbeS4(PathBuf::from(dir)));
        }
                if let Ok(dir) = std::env::var("ALPACA_PROBE_WB_SAVE") {
            sender.input(AppMsg::ProbeWBSave(PathBuf::from(dir)));
        }
        if let Ok(dir) = std::env::var("ALPACA_PROBE_WB_RESTORE") {
            sender.input(AppMsg::ProbeWBRestore(PathBuf::from(dir)));
        }
                if let Ok(dir) = std::env::var("ALPACA_PROBE_LN") {
            sender.input(AppMsg::ProbeLN(PathBuf::from(dir)));
        }

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match msg {
            AppMsg::OpenWorkspace(p) => self.open_workspace(&p, root, sender),
            AppMsg::SwitchWorkspace(i) => self.switch_to(i, root),
            AppMsg::CloseWorkspace(slot) => self.close_workspace(slot, root, sender),
            AppMsg::ShowLauncher => {
                                                self.launcher.emit(LauncherMsg::Build);
                self.stack.set_visible_child_name("launcher");
                root.set_title(Some("alpaca-code"));
            }
            AppMsg::LauncherOpen(name) => self.open_workbench(&name, sender),
            AppMsg::LauncherAdd(name) => ask_folder(root, "New Workbench", sender, Some(name)),
            AppMsg::OpenNew(p, name) => {
                                                                                                self.wb = None;
                self.wb_name = None;
                self.wb_display = Some(if name.trim().is_empty() {
                    py_basename(&p.to_string_lossy()).to_string()
                } else {
                    name
                });
                sender.input(AppMsg::OpenWorkspace(p));
            }
            AppMsg::LauncherRename { stem, display } => {
                                                                state::set_workbench_display(&stem, &display);
                if self.wb_name.as_deref() == Some(stem.as_str()) {
                    self.wb = state::load_workbench(&stem);
                }
                self.launcher.emit(LauncherMsg::Build);
            }
            AppMsg::LauncherDelete(stem) => {
                state::delete_workbench(&stem);
                if self.wb_name.as_deref() == Some(stem.as_str()) {
                                                            self.wb = None;
                    self.wb_name = None;
                    self.wb_display = None;
                }
                self.launcher.emit(LauncherMsg::Build);
            }
            AppMsg::AddProjectDialog => open_project(root, sender),
            AppMsg::OpenFile(ws, p) => {
                                if let Some(w) = self.ws.get_mut(ws).and_then(|o| o.as_mut()) {
                    w.editor.emit(EditorMsg::OpenFile(p));
                }
            }
            AppMsg::EditorState(ws, s) => {
                if let Some(w) = self.ws.get_mut(ws).and_then(|o| o.as_mut()) {
                    w.snapshot = s;
                }
                if self.live_len() > 0 {
                                                                                let sender = sender.clone();
                    glib::timeout_add(std::time::Duration::from_millis(1000), move || {
                        sender.input(AppMsg::FlushTabs);
                        glib::ControlFlow::Break
                    });
                }
            }
            AppMsg::SaveWorkbench => self.save_all(root),
            AppMsg::SaveThenClose => {
                self.save_all(root);
                root.destroy(); 
            }
            AppMsg::SaveAndQuit => {
                self.save_all(root);
                relm4::main_application().quit();
            }
            AppMsg::FlushTabs => {
                let mut st = state::load();
                for w in self.ws.iter().flatten() {
                    st = state::set_tabs(
                        st,
                        w.root.to_string_lossy().as_ref(),
                        w.snapshot.open_tabs.clone(),
                        w.snapshot.active_tab,
                    );
                }
                state::save(&st);
            }
            AppMsg::RunStatus(ws, running) => {
                                                if let Some(w) = self.ws.get_mut(ws).and_then(|o| o.as_mut()) {
                    w.running = running;
                }
                if ws == self.active {
                    self.run_btn.set_sensitive(!running);
                    self.stop_btn.set_sensitive(running);
                    self.refresh_run_style();
                }
            }
            AppMsg::RunStarting(ws) => {
                if let Some(w) = self.ws.get_mut(ws).and_then(|o| o.as_mut()) {
                    w.running = true;
                }
                if ws == self.active {
                    self.run_btn.set_sensitive(false); 
                    self.refresh_run_style();
                }
            }
            AppMsg::RunPressed => {
                                let running = self.ws.get(self.active).and_then(|o| o.as_ref()).map(|w| w.running).unwrap_or(true);
                if running {
                    return;
                }
                let cmd = self
                    .ws
                    .get(self.active)
                    .and_then(|o| o.as_ref())
                    .and_then(|w| runctl::detect(w.root.as_path()));
                let Some(cmd) = cmd else {
                    self.run_btn.set_sensitive(false);
                    self.run_btn
                        .set_tooltip_text(Some("No dev/start script or .csproj found"));
                    return;
                };
                self.stop_btn.set_sensitive(true);
                if let Some(w) = self.ws.get(self.active).and_then(|o| o.as_ref()) {
                    w.panes.emit(PanesMsg::LaunchRun { argv: cmd.argv });
                }
                                            }
            AppMsg::StopPressed => {
                                if let Some(w) = self.ws.get(self.active).and_then(|o| o.as_ref()) {
                    w.panes.emit(PanesMsg::StopRun);
                }
                self.stop_btn.set_sensitive(false);
                self.run_btn.set_sensitive(true);
                self.refresh_run_style();
            }
            AppMsg::RecentMissing(p) => {
                error_dialog(root, &format!("Project not found: {p}"), "");
            }
            AppMsg::ProjectCreated { target, res } => match res {
                Err(e) => error_dialog(root, &format!("Couldn't create {}", target.display()), &e),
                Ok(()) => sender.input(AppMsg::OpenWorkspace(target)),
            },
            AppMsg::OpenDiff { ws, rel, letter } => {
                                if let Some(w) = self.ws.get(ws).and_then(|o| o.as_ref()) {
                    w.editor.emit(EditorMsg::OpenDiff { root: w.root.clone(), rel, letter });
                }
            }
            AppMsg::Commit { ws, root, paths, msg } => {
                                                                                                                if let Some(w) = self.ws.get_mut(ws).and_then(|o| o.as_mut()) {
                    w.editor.emit(EditorMsg::SaveOpen(
                        paths.iter().map(|rel| root.join(rel)).collect(),
                    ));
                    w.pending_commit = Some((root, paths, msg));
                }
            }
            AppMsg::Flushed(ws) => {
                                                                                if let Some(w) = self.ws.get_mut(ws).and_then(|o| o.as_mut())
                    && let Some((root, paths, msg)) = w.pending_commit.take() {
                        w.gitpanel.emit(GitPanelMsg::StartCommit { root, paths, msg });
                    }
            }
            AppMsg::GitChanged(ws, rows) => {
                                                                                if let Some(w) = self.ws.get(ws).and_then(|o| o.as_ref()) {
                    let letters = rows.iter().map(|(rel, letter)| (rel.clone(), letter.clone())).collect();
                    w.editor.emit(EditorMsg::RefreshDiffs { root: w.root.clone(), letters });
                }
            }
            AppMsg::WindowActive => {
                                if let Some(w) = self.ws.get(self.active).and_then(|o| o.as_ref()) {
                    w.gitpanel.emit(GitPanelMsg::RefreshGit);
                }
            }
            AppMsg::EditOp(kind) => {
                if let Some(w) = self.ws.get_mut(self.active).and_then(|o| o.as_mut()) {
                    w.editor.emit(match kind {
                        EditKind::Undo => EditorMsg::Undo,
                        EditKind::Redo => EditorMsg::Redo,
                        EditKind::Cut => EditorMsg::Cut,
                        EditKind::Copy => EditorMsg::Copy,
                        EditKind::Paste => EditorMsg::Paste,
                        EditKind::SelectAll => EditorMsg::SelectAll,
                    });
                }
            }
            AppMsg::FocusSearch => {
                if let Some(w) = self.ws.get(self.active).and_then(|o| o.as_ref()) {
                    w.gitpanel.emit(GitPanelMsg::FocusSearch);
                }
            }
            AppMsg::ProbeS2(dir) => {
                                                                self.probe_ws = Some((dir, 0));
                let snd = sender.clone();
                glib::timeout_add(std::time::Duration::from_millis(PROBE_STEP_MS[0]), move || {
                    snd.input(AppMsg::ProbeS2Step);
                    glib::ControlFlow::Break
                });
            }
            AppMsg::ProbeS2Step => self.probe_step(root, &sender),
            AppMsg::ProbeDraw => {
                                                                                                                                if self.probe_s3.is_some() {
                    root.queue_draw();
                    PROBE_S3_TICKS.with(|t| {
                        let n = t.get() + 1;
                        t.set(n);
                                                if n % 8 == 0 {
                            println!("PROBE tick {n}: root w={}", root.allocated_width());
                        }
                    });
                }
            }
            AppMsg::ProbeS4(dir) => {
                                self.probe_s4 = Some((dir, 0));
                root.present();
                let snd = sender.clone();
                glib::timeout_add(std::time::Duration::from_millis(PROBE_S4_STEP_MS[0]), move || {
                    snd.input(AppMsg::ProbeS4Step);
                    glib::ControlFlow::Break
                });
            }
            AppMsg::ProbeS4Step => {
                let Some((ws_dir, step)) = self.probe_s4.as_ref().map(|(w, s)| (w.clone(), *s)) else {
                    return;
                };
                let say = |name: &str, ok: bool| {
                    println!("PROBE assert: {name}: {}", if ok { "PASS" } else { "FAIL" })
                };
                match step {
                    0 => {
                        self.open_workspace(ws_dir.as_path(), root, sender.clone());
                        let i = self.active;
                        sender.input(AppMsg::OpenFile(i, ws_dir.join("a.txt")));
                    }
                    1 => {
                        let ws_b = ws_dir.to_string_lossy().replace("wsA", "wsB");
                        self.open_workspace(std::path::Path::new(&ws_b), root, sender.clone());
                        sender.input(AppMsg::OpenFile(self.active, std::path::Path::new(&ws_b).join("b.txt")));
                    }
                    2 => {
                        println!("PROBE step 2: ts={}", epoch_ms());
                        self.switch_to(0, root);
                        say("two-open", self.ws.len() == 2 && self.live_len() == 2);
                                                                                                                        say(
                            "stack-visible-is-wsA",
                            self.stack
                                .visible_child()
                                .is_some_and(|v| v == self.ws[0].as_ref().unwrap().hpane),
                        );
                        say(
                            "editor-tabs-preserved",
                                                        self.ws[0].as_ref().unwrap().snapshot.open_tabs == vec!["a.txt".to_string()],
                        );
                                                                                                                                                let hw = self.ws[0].as_ref().unwrap().hpane.allocated_width();
                        let pos = self.ws[0].as_ref().unwrap().hpane.position();
                        println!("PROBE hpane-pos reads pos={pos} hpane-w={hw}");
                                                                                                let ok = if hw > 0 { pos == 1161.min(hw - 6) } else { pos == 1161 };
                        say("hpane-pos-1161", ok);
                        say(
                            "on-class-wsA",
                            self.ws[0].as_ref().unwrap().btn.has_css_class("alpaca-on")
                                && !self.ws[1].as_ref().unwrap().btn.has_css_class("alpaca-on"),
                        );
                                                                                                                                                                                                let bh = self.proj_box.allocated_height();
                        let bands: Vec<i32> =
                            self.ws.iter().flatten().map(|w| w.btn.allocated_height()).collect();
                        println!("PROBE strip: proj_box-h={} pill-band-heights={:?}", bh, bands);
                        let ok = bh > 0
                            && bands.len() == 2
                            && bands.iter().all(|h| *h == bh && *h < 32);
                        say("pill-band-floats", ok);
                                                                                                say(
                            "add-btn-icon",
                            self.add_btn.child().and_downcast::<gtk::Image>().is_some(),
                        );
                                                say(
                            "title-is-basename",
                            root.title().as_deref()
                                == Some(self.ws[0].as_ref().unwrap().root.file_name().unwrap().to_str().unwrap()),
                        );
                    }
                    3 => {
                        let ws_c = ws_dir.to_string_lossy().replace("wsA", "wsC");
                        self.open_workspace(std::path::Path::new(&ws_c), root, sender.clone());
                        say("three-open", self.ws.len() == 3 && self.live_len() == 3);
                        say("add-btn-disabled", !self.add_btn.is_sensitive());
                                                                                                                                                sender.spawn_command(move |_| {
                            let st = std::fs::read_to_string(crate::support::state::state_path())
                                .unwrap_or_default();
                            let ops = st
                                .split("\"open_projects\": [")
                                .nth(1)
                                .unwrap_or_default()
                                .split(']')
                                .next()
                                .unwrap_or_default()
                                .to_string();
                            let ok = ["wsA", "wsB", "wsC"].iter().all(|w| ops.contains(w))
                                && !st.contains("GONE");
                            println!("PROBE assert: state-open-set-live-truth: {}",
                                     if ok { "PASS" } else { "FAIL" });
                            println!("PROBE state open_projects=[{}]", ops.trim());
                        });
                                                                        self.switch_to(0, root);
                    }
                                                            4 => {
                        self.open_workspace(ws_dir.as_path(), root, sender.clone());
                        say("re-open-wsA-switches", self.live_len() == 3 && self.active == 0);
                    }
                                                                                5 => {
                        self.close_workspace(0, root, sender.clone());
                                                                        say("close-live-2", self.live_len() == 2 && self.ws.len() == 3);
                        say("close-slot-0-tombstone", self.ws[0].is_none());
                        say(
                            "close-swing-wsB",
                            self.active == 1
                                && self
                                    .stack
                                    .visible_child()
                                    .is_some_and(|v| v == self.ws[1].as_ref().unwrap().hpane),
                        );
                                                say(
                            "close-pill-class",
                            self.ws[1].as_ref().unwrap().btn.has_css_class("alpaca-on"),
                        );
                                                say("close-pill-rm", self.proj_box.observe_children().n_items() == 3);
                        say("close-reopen-plus", self.add_btn.is_sensitive());
                                                let wsb = ws_dir.to_string_lossy().replace("wsA", "wsB");
                        let wsb_state = wsb.clone(); 
                        let wsc = ws_dir.to_string_lossy().replace("wsA", "wsC");
                        sender.spawn_command(move |_| {
                            let st = std::fs::read_to_string(crate::support::state::state_path())
                                .unwrap_or_default();
                            let ops = st
                                .split("\"open_projects\": [")
                                .nth(1)
                                .unwrap_or_default()
                                .split(']')
                                .next()
                                .unwrap_or_default()
                                .to_string();
                            let ok = ops.contains(&wsb_state)
                                && ops.contains(&wsc)
                                && !ops.contains("wsA");
                            println!("PROBE assert: close-open-set-pruned: {}",
                                     if ok { "PASS" } else { "FAIL" });
                            println!("PROBE state open_projects=[{}]", ops.trim());
                        });
                                                                                                sender.input(AppMsg::OpenFile(
                            1,
                            PathBuf::from(&wsb).join("b2.txt"),
                        ));
                                                                        sender.input(AppMsg::OpenFile(0, ws_dir.join("a.txt")));
                    }
                    6 => {
                        let w = self.ws[1].as_ref().unwrap();
                        say(
                            "close-retag-proof",
                            w.snapshot.open_tabs.iter().any(|t| t == "b2.txt"),
                        );
                        println!("PROBE S4 DONE");
                        self.probe_s4 = None;
                        return;
                    }
                    _ => {}
                }
                if let Some(next) = PROBE_S4_STEP_MS.get(step + 1) {
                    self.probe_s4.as_mut().unwrap().1 = step + 1;
                    let snd = sender.clone();
                    glib::timeout_add(std::time::Duration::from_millis(*next), move || {
                        snd.input(AppMsg::ProbeS4Step);
                        glib::ControlFlow::Break
                    });
                }
            }
            AppMsg::ProbeWBSave(dir) => {
                                self.probe_wb = Some((dir, 0));
                root.present();
                let snd = sender.clone();
                glib::timeout_add(std::time::Duration::from_millis(PROBE_WB_SAVE_MS[0]), move || {
                    snd.input(AppMsg::ProbeWBStep);
                    glib::ControlFlow::Break
                });
            }
            AppMsg::ProbeWBStep => {
                let Some((ws_dir, step)) = self.probe_wb.as_ref().map(|(w, s)| (w.clone(), *s)) else {
                    return;
                };
                let say = |name: &str, ok: bool| {
                    println!("PROBE assert: {name}: {}", if ok { "PASS" } else { "FAIL" })
                };
                match step {
                    0 => {
                        self.open_workspace(ws_dir.as_path(), root, sender.clone());
                        let i = self.active;
                        sender.input(AppMsg::OpenFile(i, ws_dir.join("a.txt")));
                                                                                                root.set_size_request(1586, 992);
                    }
                    1 => {
                        let Some(w) = self.ws.get(self.active).and_then(|o| o.as_ref()) else { return };
                        w.vpane.set_position(650);
                        w.hpane.set_position(900);
                                                                                                w.gitpanel.emit(GitPanelMsg::ProbeChild(ProbeChild::Tree(
                            FileTreeMsg::RowExpanded(gtk::TreePath::from_string("1").unwrap()),
                        )));
                    }
                    2 => {
                        if let Some(w) = self.ws.get(self.active).and_then(|o| o.as_ref()) {
                            w.editor.emit(EditorMsg::ProbeScroll(0.75));
                            let sc = w.editor.model().scroll_state();
                            println!("PROBE scroll_state: {:?}", sc);
                        }
                    }
                    3 => sender.input(AppMsg::SaveWorkbench), 
                    4 => {
                                                                                                sender.spawn_command(move |_| {
                            let dir = crate::support::state::workbenches_dir();
                            let mut got: Option<String> = None;
                            if let Ok(rd) = std::fs::read_dir(&dir) {
                                for e in rd.flatten() {
                                    if e.path().extension().is_some_and(|x| x == "json") {
                                        got = std::fs::read_to_string(e.path()).ok();
                                        println!("PROBE workbench file = {}", e.path().display());
                                    }
                                }
                            }
                            let wb = got.unwrap_or_default();
                            let st = std::fs::read_to_string(crate::support::state::state_path())
                                .unwrap_or_default();
                            let ok = wb.contains("\"vpane\": 650")
                                && wb.contains("\"hpane\": 900")
                                && wb.contains("\"language\": \"rust\"")
                                && wb.contains("\"rel\": \"a.txt\"")
                                && wb.contains("sub")
                                && st.contains("\"last_workbench\": \"")
                                && st.contains("\"window\": {");
                            say("wb-save-disk", ok);
                            println!("PROBE state last_workbench={:?}",
                                     st.split("\"last_workbench\": ").nth(1)
                                       .and_then(|s| s.split(',').next()).or(Some("<none>")));
                            println!("PROBE workbench.json = {}", wb.trim());
                        });
                                                if let Some(w) = self.ws.get(self.active).and_then(|o| o.as_ref()) {
                            let (open, closed) = w.gitpanel.model().tree_state();
                            say("wb-open-has-sub", open.iter().any(|d| d.ends_with("/sub")));
                            say("wb-closed-empty", closed.is_empty());
                            println!(
                                "PROBE hpane pos={} vpane pos={}",
                                w.hpane.position(),
                                w.vpane.position()
                            );
                        }
                    }
                    5 => {
                        println!("PROBE WB SAVE DONE");
                        self.probe_wb = None;
                                                                        root.close();
                        return;
                    }
                    _ => {}
                }
                if let Some(next) = PROBE_WB_SAVE_MS.get(step + 1) {
                    self.probe_wb.as_mut().unwrap().1 = step + 1;
                    let snd = sender.clone();
                    glib::timeout_add(std::time::Duration::from_millis(*next), move || {
                        snd.input(AppMsg::ProbeWBStep);
                        glib::ControlFlow::Break
                    });
                }
            }
            AppMsg::ProbeWBRestore(dir) => {
                                                self.probe_wbr = Some((dir, 0));
                root.present();
                let snd = sender.clone();
                glib::timeout_add(std::time::Duration::from_millis(PROBE_WB_RESTORE_MS[0]), move || {
                    snd.input(AppMsg::ProbeWBRStep);
                    glib::ControlFlow::Break
                });
            }
            AppMsg::ProbeWBRStep => {
                let Some((ws_dir, step)) = self.probe_wbr.as_ref().map(|(w, s)| (w.clone(), *s)) else {
                    return;
                };
                let say = |name: &str, ok: bool| {
                    println!("PROBE assert: {name}: {}", if ok { "PASS" } else { "FAIL" })
                };
                match step {
                    0 => {
                                                                                                                        self.launcher.model().probe_click(0);
                    }
                    1 => {
                                                say("r-strip", self.live_len() == 1 && self.active == 0);
                        say("r-page-flipped", self.stack.visible_child_name().as_deref() != Some("launcher"));
                        say(
                            "r-adopted",
                            self.wb_name.as_deref()
                                == self.launcher.model().entries.first().map(|e| e.name.as_str()),
                        );
                        if let Some(w) = self.ws.get(0).and_then(|o| o.as_ref()) {
                            say("r-tabs", w.snapshot.open_tabs == vec!["a.txt".to_string()]);
                                                                                                                let _ = &ws_dir;
                            let hp = w.hpane.position();
                            let ha = w.hpane.allocated_width();
                            let vp = w.vpane.position();
                            let vh = w.vpane.allocated_height();
                            say(
                                "r-hpane",
                                ha <= 0 && hp == 900 || ha > 0 && hp == (900).min(ha - 6),
                            );
                            say(
                                "r-vpane",
                                vh <= 0 && vp == 650 || vh > 0 && vp == (650).min(vh - 6),
                            );
                                                                                                                let sc = w.editor.model().scroll_state();
                            say(
                                "r-scroll",
                                sc.as_ref().map(|(r, f)| r == "a.txt" && *f > 0.4)
                                    .unwrap_or(false),
                            );
                            println!("PROBE restore hpane {hp}/{ha} vpane {vp}/{vh} scroll {sc:?}");
                                                        let (open, _closed) = w.gitpanel.model().tree_state();
                            say("r-open-has-sub", open.iter().any(|d| d.ends_with("/sub")));
                        }
                    }
                    2 => {
                        println!("PROBE WB RESTORE DONE");
                        self.probe_wbr = None;
                        return;
                    }
                    _ => {}
                }
                if let Some(next) = PROBE_WB_RESTORE_MS.get(step + 1) {
                    self.probe_wbr.as_mut().unwrap().1 = step + 1;
                    let snd = sender.clone();
                    glib::timeout_add(std::time::Duration::from_millis(*next), move || {
                        snd.input(AppMsg::ProbeWBRStep);
                        glib::ControlFlow::Break
                    });
                }
            }
            AppMsg::ProbeLN(dir) => {
                                self.probe_l = Some((dir, 0));
                root.present();
                let snd = sender.clone();
                glib::timeout_add(std::time::Duration::from_millis(PROBE_LN_MS[0]), move || {
                    snd.input(AppMsg::ProbeLNStep);
                    glib::ControlFlow::Break
                });
            }
            AppMsg::ProbeLNStep => {
                                                let Some((_rig, step)) = self.probe_l.as_ref().map(|(w, s)| (w.clone(), *s)) else {
                    return;
                };
                let say = |name: &str, ok: bool| {
                    println!("PROBE assert: {name}: {}", if ok { "PASS" } else { "FAIL" })
                };
                                                                                match step {
                    0 => {
                                                println!("PROBE LN: boot asserts");
                        say(
                            "ln-boot-page",
                            self.stack.visible_child_name().as_deref() == Some("launcher"),
                        );
                        let texts = self.launcher.model().probe_texts();
                        say("ln-cards", texts.len() == 2);
                        say(
                            "ln-order-last-saved-first",
                            texts.len() == 2
                                && texts[0].0 == "aaaabbbb-0001"
                                && texts[1].0 == "aaaabbbb-0002",
                        );
                        say(
                            "ln-tile0-facts",
                            texts.first().is_some_and(|(_, t)| {
                                t.first().is_some_and(|x| x.0 == "projA" && x.1 == "C#" && x.2 == "main")
                            }),
                        );
                        say(
                            "ln-tile1-facts",
                            texts.first().is_some_and(|(_, t)| {
                                t.get(1).is_some_and(|x| {
                                    x.0 == "projB" && x.1 == "TS" && x.2 == "feature-ln"
                                })
                            }),
                        );
                                                say(
                            "ln-chrome-hidden",
                            !self.menu_btn.is_visible() && !self.add_btn.is_visible(),
                        );
                                                                                                                        say("ln-boot-stale-no-adopt", self.wb_name.is_none());
                    }
                    1 => {
                                                                        self.launcher.model().probe_click(0);
                    }
                    2 => {
                        println!("PROBE LN: open asserts");
                                                                        say("ln-open-two", self.live_len() == 2 && self.active == 1);
                        say(
                            "ln-workbench-adopted",
                            self.wb_name.as_deref() == Some("aaaabbbb-0001"),
                        );
                        say(
                            "ln-page-flipped",
                            self.stack.visible_child_name().as_deref() != Some("launcher"),
                        );
                        say("ln-title", root.title().as_deref() == Some("projB"));
                                                say(
                            "ln-chrome-shown",
                            self.menu_btn.is_visible() && self.add_btn.is_visible(),
                        );
                    }
                    3 => {
                                                self.launcher.model().probe_click(1);
                    }
                    4 => {
                                                                        say("ln-blend-switch", self.live_len() == 2);
                        say("ln-blend-active", self.active == 1);
                        say(
                            "ln-workbench-swapped",
                            self.wb_name.as_deref() == Some("aaaabbbb-0002"),
                        );
                    }
                    5 => {
                        println!("PROBE LN: back to hub");
                        sender.input(AppMsg::ShowLauncher);
                    }
                    6 => {
                                                                                                                                                println!("PROBE LN: rename drive");
                        say(
                            "ln-rename-pop-entry",
                            self.launcher.model().probe_pencil(0).is_some_and(|e| {
                                e.set_text("Nice wb");
                                true
                            }),
                        );
                    }
                    7 => {
                                                                                                println!("PROBE LN: rename commit");
                        let ok = self
                            .launcher
                            .model()
                            .probe_pencil_entry(0)
                            .is_some_and(|e| {
                                e.emit_activate();
                                true
                            });
                        say("ln-rename-commit", ok);
                                                                                                say(
                            "ln-rename-commit-adopted",
                            self.launcher.model().probe_pencil(1).is_some_and(|e| {
                                e.set_text("Adopted 0002");
                                e.emit_activate();
                                true
                            }),
                        );
                    }
                    8 => {
                        println!("PROBE LN: rename asserts");
                                                                        let texts = self.launcher.model().probe_texts();
                        say("ln-rename-label", texts.first().is_some_and(|(n, _)| n == "Nice wb"));
                        say(
                            "ln-rename-file",
                            state::load_workbench("aaaabbbb-0001")
                                .is_some_and(|w| w.display.as_deref() == Some("Nice wb")),
                        );
                                                                        say(
                            "ln-rename-adopt-file",
                            state::load_workbench("aaaabbbb-0002")
                                .is_some_and(|w| w.display.as_deref() == Some("Adopted 0002")),
                        );
                        say(
                            "ln-rename-adopt-base",
                            self.wb.as_ref().and_then(|w| w.display.as_deref())
                                == Some("Adopted 0002"),
                        );
                        say(
                            "ln-rename-adopt-untouched",
                            self.wb_name.as_deref() == Some("aaaabbbb-0002"),
                        );
                                                say(
                            "ln-rename-page",
                            self.stack.visible_child_name().as_deref() == Some("launcher"),
                        );
                    }
                    9 => {
                                                                                                println!("PROBE LN: delete drive");
                        say(
                            "ln-del-pop",
                            self.launcher.model().probe_trash(1)
                                && self.launcher.model().probe_delete_ok(1),
                        );
                    }
                    10 => {
                        println!("PROBE LN: delete asserts");
                        let texts = self.launcher.model().probe_texts();
                                                                                                say("ln-del-card-gone", texts.len() == 1 && texts[0].0 == "Nice wb");
                        say(
                            "ln-del-file-gone",
                            state::load_workbench("aaaabbbb-0002").is_none(),
                        );
                        say("ln-del-adopt-cleared", self.wb_name.is_none() && self.wb.is_none());
                                                say("ln-del-fresh-rearm", self.wb_display.is_none());
                    }
                    11 => {
                                                                                                println!("PROBE LN: plus click");
                        self.launcher.emit(LauncherMsg::PlusClick);
                    }
                    12 => {
                        println!("PROBE LN: plus entry asserts");
                        let e = self.launcher.model().probe_plus_entry();
                        say("ln-plus-entry", e.is_some());
                        if let Some(ent) = e {
                            say("ln-plus-entry-empty", ent.text().is_empty());
                        }
                                                let rig = self
                            .probe_l
                            .as_ref()
                            .map(|(d, _)| d.clone())
                            .unwrap_or_default();
                        sender.input(AppMsg::OpenNew(rig.join("projA"), "My New Wb".into()));
                    }
                    13 => {
                        println!("PROBE LN: create asserts");
                                                                                                say("ln-new-switch", self.live_len() == 2);
                        say("ln-new-identity", self.wb_name.is_none() && self.wb.is_none());
                        say(
                            "ln-new-display",
                            self.wb_display.as_deref() == Some("My New Wb"),
                        );
                    }
                    14 => {
                        println!("PROBE LN DONE");
                        self.probe_l = None;
                        return;
                    }
                    _ => {}
                }
                                                                                                if let Some(n) = std::env::var("ALPACA_PROBE_LN_HOLD")
                    .ok()
                    .and_then(|s| s.parse::<usize>().ok())
                    && step + 1 >= n
                {
                    println!("PROBE LN HOLD: parked after step {step} — window open for capture");
                    self.probe_l = None;
                    return;
                }
                if let Some(next) = PROBE_LN_MS.get(step + 1) {
                    self.probe_l.as_mut().unwrap().1 = step + 1;
                    let snd = sender.clone();
                    glib::timeout_add(std::time::Duration::from_millis(*next), move || {
                        snd.input(AppMsg::ProbeLNStep);
                        glib::ControlFlow::Break
                    });
                }
            }
            AppMsg::ProbeS3(dir) => {
                                                                self.probe_s3 = Some((dir, 0));
                                root.present();
                let snd = sender.clone();
                glib::timeout_add(std::time::Duration::from_millis(60), move || {
                    snd.input(AppMsg::ProbeDraw);
                    glib::ControlFlow::Continue
                });
                let snd = sender.clone();
                glib::timeout_add(std::time::Duration::from_millis(PROBE_S3_STEP_MS[0]), move || {
                    snd.input(AppMsg::ProbeS3Step);
                    glib::ControlFlow::Break
                });
            }
            AppMsg::ProbeS3Step => {
                                                                let Some((ws_dir, step)) = self.probe_s3.as_ref().map(|(w, s)| (w.clone(), *s)) else {
                    return;
                };
                println!("PROBE step {}: ts={}", step, epoch_ms());
                                                                match step {
                    0 => {
                        self.open_workspace(ws_dir.as_path(), root, sender.clone());
                        let i = self.active;
                        for f in ["package-lock.json", "next.config.ts", "components.json",
                                  "related-products.tsx", "trude.json",
                                  "deep/path/segments/segment-file-with-long-name.tsx"] {
                            sender.input(AppMsg::OpenFile(i, ws_dir.join(f)));
                        }
                    }
                    1 => {
                        let Some(w) = self.ws.get(self.active).and_then(|o| o.as_ref()) else { return };
                        let checks = [
                            ("window", root.measure(gtk::Orientation::Horizontal, -1).0, 560),
                            ("editor", w.editor.widget().measure(gtk::Orientation::Horizontal, -1).0, 300),
                            ("panes", w.panes.widget().measure(gtk::Orientation::Horizontal, -1).0, 300),
                        ];
                        for (name, got, cap) in checks {
                            let ok = got <= cap;
                            println!("PROBE assert: s3-min-{name} {got}<={cap}: {}",
                                     if ok { "PASS" } else { "FAIL" });
                        }
                        w.gitpanel.emit(GitPanelMsg::TreeMin);
                    }
                    2 => {
                                                                                                                                                                        root.set_size_request(1586, 992);
                    }
                    3 => if let Some(w) = self.ws.get(self.active).and_then(|o| o.as_ref()) { w.hpane.set_position(900); },
                    4 => {
                        let Some(w) = self.ws.get(self.active).and_then(|o| o.as_ref()) else { return };
                        println!("PROBE hpane pos={} w={} at wide-trace",
                                 w.hpane.position(), w.hpane.allocated_width());
                        w.editor.emit(EditorMsg::ProbeTrace("wide".into()));
                    }
                    5 => if let Some(w) = self.ws.get(self.active).and_then(|o| o.as_ref()) { w.hpane.set_position(850); },
                    6 => {
                        if let Some(w) = self.ws.get(self.active).and_then(|o| o.as_ref()) {
                            w.editor.emit(EditorMsg::ProbeTrace("narrow".into()));
                        }
                        println!("PROBE S3 DONE");
                        self.probe_s3 = None;
                        return;
                    }
                    _ => {} 
                }
                                if let Some(next) = PROBE_S3_STEP_MS.get(step + 1) {
                    self.probe_s3.as_mut().unwrap().1 = step + 1;
                    let snd = sender.clone();
                    glib::timeout_add(std::time::Duration::from_millis(*next), move || {
                        snd.input(AppMsg::ProbeS3Step);
                        glib::ControlFlow::Break
                    });
                }
            }
        }
    }
}

const PROBE_STEP_MS: [u64; 15] = [
    300, 500, 450, 350, 400, 400, 400, 400, 400, 400, 400, 300, 300, 800, 2800,
];

const PROBE_S3_STEP_MS: [u64; 7] = [500, 700, 700, 700, 600, 600, 600];

const PROBE_S4_STEP_MS: [u64; 8] = [500, 700, 700, 700, 700, 500, 700, 700];

const PROBE_WB_SAVE_MS: [u64; 6] = [500, 700, 700, 700, 700, 700];
const PROBE_LN_MS: [u64; 15] = [
    1300, 800, 800, 800, 800, 700, 900, 900, 900, 900, 900, 900, 900, 900, 700,
];
const PROBE_WB_RESTORE_MS: [u64; 3] = [500, 1400, 700];

pub(crate) fn epoch_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn step_ms(i: usize) -> u64 {
    PROBE_STEP_MS.get(i).copied().unwrap_or(0)
}

impl App {
            fn probe_step(&mut self, root: &gtk::ApplicationWindow, sender: &ComponentSender<Self>) {
                const PROBE_MSG: &str = "s2 probe commit";
        let Some((ws, step)) = self.probe_ws.as_ref().map(|(w, s)| (w.clone(), *s)) else {
            return;
        };
                let ws_b = ws.to_string_lossy().replace("wsA", "wsB");
                println!("PROBE step {}: ts={}", step, epoch_ms());
        match step {
                                    0 => {
                self.open_workspace(ws.as_path(), root, sender.clone());
                let i = self.active;
                for f in ["package-lock.json", "next.config.ts", "components.json",
                          "related-products.tsx", "trude.json",
                          "deep/path/segments/segment-file-with-long-name.tsx"] {
                    sender.input(AppMsg::OpenFile(i, ws.join(f)));
                }
            }
            1 => {
                if let Some(w) = self.ws.get(self.active).and_then(|o| o.as_ref()) {
                    w.gitpanel.emit(GitPanelMsg::ModeChanges);
                }
            }
                        2 => {
                if let Some(w) = self.ws.get(self.active).and_then(|o| o.as_ref()) {
                    w.gitpanel.emit(GitPanelMsg::ProbeChild(ProbeChild::Changes(
                        GitViewMsg::ProbeMsg(PROBE_MSG.into()),
                    )));
                }
            }
                                    3 => sender.input(AppMsg::OpenDiff {
                ws: self.active,
                rel: "a.txt".into(),
                letter: "M".into(),
            }),
            4 => {
                if let Some(w) = self.ws.get(self.active).and_then(|o| o.as_ref()) {
                    w.editor.emit(EditorMsg::ProbeClip);
                }
            }
                                                                        5 => {
                if let Some(w) = self.ws.get(self.active).and_then(|o| o.as_ref()) {
                    w.gitpanel.emit(GitPanelMsg::ProbeEntry("alpha".into()));
                }
            }
            6 => {
                if let Some(w) = self.ws.get(self.active).and_then(|o| o.as_ref()) {
                    w.gitpanel.emit(GitPanelMsg::ProbeEntry("alphabet".into()));
                }
            }
            7 => {
                if let Some(w) = self.ws.get(self.active).and_then(|o| o.as_ref()) {
                    w.gitpanel.emit(GitPanelMsg::ProbeEntry("alphabetZ".into()));
                }
            }
            8 => {
                if let Some(w) = self.ws.get(self.active).and_then(|o| o.as_ref()) {
                    w.gitpanel.emit(GitPanelMsg::ProbeEntry("alphabetZx".into()));
                }
            }
                                                                        9 => {
                if let Some(w) = self.ws.get(self.active).and_then(|o| o.as_ref()) {
                    w.gitpanel.emit(GitPanelMsg::ProbeEntry("".into()));
                    w.editor.emit(EditorMsg::ProbeDiffScroll);
                }
            }
                                                                        10 => {
                if let Some(w) = self.ws.get(self.active).and_then(|o| o.as_ref()) {
                    let mast = || GitPanelMsg::ProbeChild(ProbeChild::Changes(
                        GitViewMsg::RowToggled(gtk::TreePath::from_string("0").unwrap()),
                    ));
                    w.gitpanel.emit(mast()); 
                    w.gitpanel.emit(mast()); 
                }
            }
                                    11 => {
                if let Some(w) = self.ws.get(self.active).and_then(|o| o.as_ref()) {
                    w.gitpanel.emit(GitPanelMsg::ProbeChild(ProbeChild::Changes(
                        GitViewMsg::CommitClicked,
                    )));
                }
            }
                                                            12 => self.open_workspace(Path::new(&ws_b), root, sender.clone()),
                        13 => {
                let ws_a = ws.clone();
                let wsb: PathBuf = ws_b.clone().into();
                sender.spawn_command(move |_| {
                    let git = |dir: &Path, args: &[&str]| {
                        let o = std::process::Command::new("git")
                            .args(args)
                            .current_dir(dir)
                            .output();
                        o.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                            .unwrap_or_default()
                    };
                    let say = |name: &str, ok| {
                        println!("PROBE assert: {}: {}", name, if ok { "PASS" } else { "FAIL" })
                    };
                                                                                say(
                        "wsA-commit-count",
                        git(&ws_a, &["rev-list", "--count", "HEAD"]) == "2",
                    );
                    say(
                        "wsA-commit-msg",
                        git(&ws_a, &["log", "-1", "--format=%s"]) == PROBE_MSG,
                    );
                    say("wsB-commit-count", git(&wsb, &["rev-list", "--count", "--all"]) == "0");
                                                            say(
                        "wsA-pushed",
                        git(&ws_a, &["rev-parse", "origin/main"])
                            == git(&ws_a, &["rev-parse", "HEAD"]),
                    );
                    let st = std::fs::read_to_string(state::state_path()).unwrap_or_default();
                    say(
                        "state-last-project",
                        st.contains(&format!("\"last_project\": \"{}\"", ws_b)),
                    );
                                                        });
            }
            14 => {
                println!("PROBE S2 DONE");
                self.probe_ws = None;
                return;
            }
            _ => return,
        }
                                        if let Some(n) = std::env::var("ALPACA_PROBE_S2_HOLD")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            && step + 1 >= n
        {
            println!("PROBE S2 HOLD: parked after step {step} — window open for capture");
            self.probe_ws = None;
            return;
        }
        self.probe_ws.as_mut().unwrap().1 = step + 1;
        let ms = step_ms(step + 1);
        let snd = sender.clone();
        glib::timeout_add(std::time::Duration::from_millis(ms), move || {
            snd.input(AppMsg::ProbeS2Step);
            glib::ControlFlow::Break
        });
    }

                        fn open_workspace(
        &mut self,
        path: &Path,
        root_win: &gtk::ApplicationWindow,
        sender: ComponentSender<Self>,
    ) {
                                let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
                        let open_slots: Vec<usize> = self.live_slots();
        let open: Vec<PathBuf> = open_slots
            .iter()
            .filter_map(|s| self.ws[*s].as_ref().map(|w| w.root.clone()))
            .collect();
        match add_decision(&open, &path) {
            AddDecision::Full => {
                error_dialog(root_win, "Maximum of 3 projects open", "");
            }
            AddDecision::Switch(i) => self.switch_to(open_slots[i], root_win),
            AddDecision::Add => {
                let i = self.ws.len(); 
                let sess = self
                    .wb
                    .as_ref()
                    .and_then(|w| w.projects.get(path.to_string_lossy().as_ref()))
                    .cloned();
                let ws = Workspace::new(path.to_path_buf(), i, sender.clone(), sess);
                                self.stack
                    .add_named(&ws.hpane, Some(&ws.root.to_string_lossy()));
                self.ws.push(Some(ws));
                                                                                self.proj_box.remove(&self.add_btn);
                self.proj_box.append(&self.ws[i].as_ref().unwrap().btn);
                self.proj_box.append(&self.add_btn);
                                self.add_btn.set_sensitive(self.live_len() < MAX_WORKSPACES);
                let path_str = path.to_string_lossy();
                                                                                                                let mut st = state::remember(state::load(), path_str.as_ref());
                st.open_projects = self
                    .ws
                    .iter()
                    .flatten()
                    .map(|w| w.root.to_string_lossy().into_owned())
                    .collect();
                state::save(&st);
                self.switch_to(i, root_win);
            }
        }
    }

                            fn open_workbench(&mut self, name: &str, sender: ComponentSender<Self>) {
        let Some(wb) = state::load_workbench(name) else {
            return;
        };
        let roots = crate::ui::launcher::open_roots(
            &wb,
            state::load().last_project.as_deref().map(Path::new),
        );
        if roots.is_empty() {
            return; 
        }
        self.wb = Some(wb);
        self.wb_name = Some(name.to_owned());
        for p in roots {
            sender.input(AppMsg::OpenWorkspace(p));
        }
    }

                fn switch_to(&mut self, i: usize, root: &gtk::ApplicationWindow) {
        let Some(w) = self.ws.get(i).and_then(|o| o.as_ref()) else {
            return;
        };
        let path_str = w.root.to_string_lossy().into_owned();
        let hpane = w.hpane.clone(); 
        self.stack.set_visible_child(&hpane);
        self.active = i;
                root.set_title(Some(&py_basename(&path_str)));
        state::save(&state::remember(state::load(), &path_str));
        for (j, w) in self.ws.iter().enumerate() {
            if let Some(w) = w.as_ref() {
                if j == i {
                    w.btn.add_css_class("alpaca-on");
                } else {
                    w.btn.remove_css_class("alpaca-on");
                }
            }
        }
                self.sync_run_buttons();
                if let Some(w) = self.ws.get(i).and_then(|o| o.as_ref()) {
            w.gitpanel.emit(GitPanelMsg::RefreshGit);
        }
    }

        fn live_len(&self) -> usize {
        self.ws.iter().flatten().count()
    }

                    fn save_all(&mut self, root_win: &gtk::ApplicationWindow) {
        let mut st = state::load();
        for w in self.ws.iter().flatten() {
            st = state::set_tabs(
                st,
                w.root.to_string_lossy().as_ref(),
                w.snapshot.open_tabs.clone(),
                w.snapshot.active_tab,
            );
        }
        st.window = Some(state::WindowGeom {
            width: root_win.width(),
            height: root_win.height(),
            maximized: root_win.is_maximized(),
        });
        let minted = self.wb_name.is_none();
        let name = self
            .wb_name
            .get_or_insert_with(|| glib::uuid_string_random().to_string())
            .clone();
                                let display = if minted {
            self.wb_display.take()
        } else {
            None
        };
        let mut wb = state::WorkbenchFile {
            name: name.clone(),
            display,
            projects: Default::default(),
        };
                                if let Some(old) = self.wb.take() {
            wb.projects = old.projects;
            if !minted {
                wb.display = old.display; 
            }
        }
        for w in self.ws.iter().flatten() {
                                    let (expanded, closed) = w.gitpanel.model().tree_state();
            wb.projects.insert(
                w.root.to_string_lossy().into_owned(),
                state::ProjectSession {
                    language: crate::fs::filetree::dominant_language(&w.root),
                    vpane: w.vpane.position(),
                    hpane: w.hpane.position(),
                    scroll: w
                        .editor
                        .model()
                        .scroll_state()
                        .map(|(rel, frac)| state::ScrollPos { rel, frac }),
                    expanded,
                    changes_closed: closed,
                },
            );
        }
        state::save_workbench_to(&wb, &state::workbench_path(&name));
        st.last_workbench = Some(name);
        state::save(&st);
        self.wb = Some(wb); 
    }

            fn live_slots(&self) -> Vec<usize> {
        self.ws
            .iter()
            .enumerate()
            .filter_map(|(s, w)| w.is_some().then_some(s))
            .collect()
    }

                                    fn close_workspace(&mut self, slot: usize, root: &gtk::ApplicationWindow, sender: ComponentSender<Self>) {
        let Some(w) = self.ws.get_mut(slot).and_then(|o| o.take()) else {
            return;
        };
                                                for pid in w.panes.model().live_pids() {
            kill_tree(pid);
        }
        self.stack.remove(&w.hpane);
        self.proj_box.remove(&w.btn);
        drop(w); 
                        if slot == self.active {
            match close_target(&self.live_slots(), slot) {
                Some(next) => self.switch_to(next, root),
                None => {
                                                                                                    self.active = 0;
                    let mut st = state::load();
                    st.last_project = None;
                    st.open_projects = Vec::new();
                    state::save(&st);
                    sender.input(AppMsg::ShowLauncher);
                }
            }
        }
        self.add_btn.set_sensitive(self.live_len() < MAX_WORKSPACES);
                        if !self.live_slots().is_empty() {
            let mut st = state::load();
            st.open_projects = self
                .ws
                .iter()
                .flatten()
                .map(|w| w.root.to_string_lossy().into_owned())
                .collect();
            state::save(&st);
        }
    }

            fn sync_run_buttons(&mut self) {
        let Some(w) = self.ws.get(self.active).and_then(|o| o.as_ref()) else {
            self.run_btn.set_sensitive(false);
            self.stop_btn.set_sensitive(false);
            self.refresh_run_style();
            return;
        };
        let cmd = runctl::detect(w.root.as_path());
        self.run_btn.set_tooltip_text(Some(
            cmd.as_ref()
                .map(|c| c.label.as_str())
                .unwrap_or("No dev/start script or .csproj found"),
        ));
        self.run_btn.set_sensitive(!w.running);
        self.stop_btn.set_sensitive(w.running);
        self.refresh_run_style();
    }

        fn refresh_run_style(&self) {
        let running = self.ws.get(self.active).and_then(|o| o.as_ref()).map(|w| w.running).unwrap_or(false);
        if running {
            self.run_row.set_css_classes(&["alpaca-runpill", "running"]);
        } else {
            self.run_row.set_css_classes(&["alpaca-runpill"]);
        }
    }
}

pub(crate) fn py_basename(s: &str) -> String {
    let t = s.trim_end_matches('/');
    t.rsplit('/').find(|c| !c.is_empty()).unwrap_or(t).to_owned()
}

fn clamp_to_monitor(w: i32, h: i32) -> (i32, i32) {
    if let Some(disp) = gdk::Display::default() {
        let mons = disp.monitors();
        if mons.n_items() > 0
            && let Some(mon) = mons.item(0).and_downcast::<gdk::Monitor>() {
                let g = mon.geometry();
                return (w.min(g.width() - 40), h.min(g.height() - 140));
            }
    }
    (w, h)
}

fn workbench_menu() -> gio::Menu {
    let m = gio::Menu::new();
    let sec = gio::Menu::new();
    sec.append(Some("New Workbench"), Some("app.new-workbench"));
    sec.append(Some("Open Workbench"), Some("app.open-workbench"));
    m.append_section(None, &sec);
    let sec = gio::Menu::new();
    sec.append(Some("Save Workbench"), Some("app.save-workbench"));
    m.append_section(None, &sec);
    let sec = gio::Menu::new();
    sec.append(Some("Exit"), Some("app.quit"));
    m.append_section(None, &sec);
    m
}

fn ask_folder(
    root_win: &gtk::ApplicationWindow,
    title: &str,
    sender: ComponentSender<App>,
            fresh: Option<String>,
) {
    let dialog = gtk::FileChooserNative::new(
        Some(title),
        Some(root_win),
        gtk::FileChooserAction::SelectFolder,
        Some("Open"),
        Some("Cancel"),
    );
    let sender = sender.clone();
    let fresh_c = fresh.clone();
    dialog.connect_response(move |d, resp| {
        let path = if resp == gtk::ResponseType::Accept {
            d.file().and_then(|f| f.path())
        } else {
            None
        };
        d.destroy();
        if let Some(p) = path
            && p.is_dir() {
                if let Some(name) = fresh_c.clone() {
                    sender.input(AppMsg::OpenNew(p, name));
                } else {
                    sender.input(AppMsg::OpenWorkspace(p));
                }
            }
    });
    dialog.show();
}

fn open_project(root_win: &gtk::ApplicationWindow, sender: ComponentSender<App>) {
    ask_folder(root_win, "Open Project", sender, None);
}

fn new_project(root_win: &gtk::ApplicationWindow, sender: ComponentSender<App>) {
    let d = gtk::Dialog::builder().title("New Project").modal(true).build();
    d.set_transient_for(Some(root_win));
    d.set_default_size(480, 160);
    let name_e = gtk::Entry::builder().placeholder_text("project name").build();
    let parent_e = gtk::Entry::new();
    parent_e.set_text(&format!("{}/FunProjects", glib::home_dir().to_string_lossy()));
    name_e.set_hexpand(true);
    parent_e.set_hexpand(true);
    d.content_area().append(&name_e);
    d.content_area().append(&parent_e);
    d.add_button("Create", gtk::ResponseType::Accept);
    d.add_button("Cancel", gtk::ResponseType::Cancel);
    d.connect_response(move |dd, resp| {
        let name = name_e.text().trim().to_owned();
        let parent = parent_e.text().trim().to_owned();
        dd.destroy();
        if resp != gtk::ResponseType::Accept || name.is_empty() {
            return;
        }
        let target = Path::new(&parent).join(name.as_str());
                                        let snd = sender.clone();
        std::thread::spawn(move || {
            let res = runctl::create_project(&target);
            snd.input(AppMsg::ProjectCreated { target, res });
        });
    });
    d.present();
}

fn error_dialog(root: &gtk::ApplicationWindow, primary: &str, secondary: &str) {
    show_message(root, primary, Some(secondary), gtk::ButtonsType::Close);
}

fn show_message(
    root: &gtk::ApplicationWindow,
    text: &str,
    secondary: Option<&str>,
    buttons: gtk::ButtonsType,
) -> gtk::MessageDialog {
    #[allow(deprecated)]
    let d = gtk::MessageDialog::new(
        Some(root),
        gtk::DialogFlags::MODAL,
        gtk::MessageType::Info,
        buttons,
        text,
    );
    if let Some(sec) = secondary {
        #[allow(deprecated)]
        d.set_secondary_text(Some(sec));
    }
    d.connect_response(|dd, _r| dd.destroy());
    d.present();
    d
}

fn register_actions(
    app: &gtk::Application,
    root_win: &gtk::ApplicationWindow,
    sender: ComponentSender<App>,
) {
    {
        let act = gio::SimpleAction::new("open-project", None);
        let root = root_win.clone();
        let sender = sender.clone();
        act.connect_activate(move |_a, _p| open_project(&root, sender.clone()));
        app.add_action(&act);
    }

    {
        let act = gio::SimpleAction::new("new-project", None);
        let root = root_win.clone();
        let sender = sender.clone();
        act.connect_activate(move |_a, _p| new_project(&root, sender.clone()));
        app.add_action(&act);
    }

    let quit = gio::SimpleAction::new("quit", None);
    {
                        let sender = sender.clone();
        quit.connect_activate(move |_a, _p| sender.input(AppMsg::SaveAndQuit));
    }
    app.add_action(&quit);

                {
        let sender = sender.clone();
        let act = gio::SimpleAction::new("new-workbench", None);
        act.connect_activate(move |_a, _p| sender.input(AppMsg::ShowLauncher));
        app.add_action(&act);
    }
    {
        let sender = sender.clone();
        let act = gio::SimpleAction::new("open-workbench", None);
        act.connect_activate(move |_a, _p| sender.input(AppMsg::ShowLauncher));
        app.add_action(&act);
    }
    {
                let sender = sender.clone();
        let a = gio::SimpleAction::new("save-workbench", None);
        a.connect_activate(move |_a, _p| sender.input(AppMsg::SaveWorkbench));
        app.add_action(&a);
    }
    {
                        let sender = sender.clone();
        let a = gio::SimpleAction::new("focus-search", None);
        a.connect_activate(move |_a, _p| sender.input(AppMsg::FocusSearch));
        app.add_action(&a);
    }

                
            for name in ["undo", "redo", "cut", "copy", "paste", "select-all"] {
        let a = gio::SimpleAction::new(name, None);
        let sender = sender.clone();
        a.connect_activate(move |_a, _p| {
            let kind = match name {
                "undo" => EditKind::Undo,
                "redo" => EditKind::Redo,
                "cut" => EditKind::Cut,
                "copy" => EditKind::Copy,
                "paste" => EditKind::Paste,
                _ => EditKind::SelectAll,
            };
            sender.input(AppMsg::EditOp(kind));
        });
        app.add_action(&a);
    }

    let minimize = gio::SimpleAction::new("win-minimize", None);
    {
        let w = root_win.clone();
        minimize.connect_activate(move |_a, _p| w.minimize());
    }
    app.add_action(&minimize);

    let close = gio::SimpleAction::new("close", None);
    {
                        let sender = sender.clone();
        close.connect_activate(move |_a, _p| sender.input(AppMsg::SaveThenClose));
    }
    app.add_action(&close);

    let about = gio::SimpleAction::new("about", None);
    {
        let w = root_win.clone();
        about.connect_activate(move |_a, _p| {
            gtk::AboutDialog::builder()
                .transient_for(&w)
                .modal(true)
                .program_name("alpaca-code")
                .version("1.0")
                .comments("Minimal agentic IDE — GTK4 wrapper for the claude CLI")
                .build()
                .present();
        });
    }
    app.add_action(&about);

    let recents = gio::SimpleAction::new("open-recent", Some(glib::VariantTy::STRING));
    {
        let sender = sender.clone();
        recents.connect_activate(move |_a, param| {
            let p = param.and_then(|v| v.get::<String>()).unwrap_or_default();
            if Path::new(&p).is_dir() {
                sender.input(AppMsg::OpenWorkspace(p.into()));
            } else {
                sender.input(AppMsg::RecentMissing(p));
            }
        });
    }
    app.add_action(&recents);

    app.set_accels_for_action("app.quit", &["<Control>q"]);
    app.set_accels_for_action("app.save-workbench", &["<Control><Shift>s"]);
    app.set_accels_for_action("app.focus-search", &["<Control><Shift>f"]);
}

pub fn run(root: Option<String>) {
    let root: Option<PathBuf> = root.filter(|p| Path::new(p).is_dir()).map(Into::into);
            let relm = relm4::RelmApp::new("io.alpaca.rs").with_args(vec![]);
    crate::render::style::init();
    relm.run::<App>(root);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pb(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn close_target_swings_right_then_left() {
                                assert_eq!(close_target(&[0, 1], 0), Some(1));
        assert_eq!(close_target(&[0, 1], 1), Some(0));
        assert_eq!(close_target(&[0, 1, 2], 1), Some(2));
        assert_eq!(close_target(&[0, 1, 2], 2), Some(1));
                assert_eq!(close_target(&[1, 3], 1), Some(3));
        assert_eq!(close_target(&[1, 3], 3), Some(1));
                assert_eq!(close_target(&[2], 2), None);
    }

    #[test]
    fn add_decision_switch_add_full() {
                        let open = [pb("/a"), pb("/b")];
        assert_eq!(add_decision(&open, Path::new("/b")), AddDecision::Switch(1));
        assert_eq!(add_decision(&open, Path::new("/c")), AddDecision::Add);
        let three = [pb("/a"), pb("/b"), pb("/c")];
        assert_eq!(add_decision(&three, Path::new("/d")), AddDecision::Full);
                assert_eq!(add_decision(&[], Path::new("/a")), AddDecision::Add);
    }

        fn attr(model: &impl glib::object::IsA<gio::MenuModel>, name: &str, i: i32) -> Option<String> {
        use gio::prelude::MenuModelExt;
        model
            .item_attribute_value(i, name, None)
            .and_then(|v| v.get::<String>())
    }

    #[test]
    fn workbench_menu_sections_labels_actions() {
        use gio::prelude::MenuModelExt;
                let m = workbench_menu();
        assert_eq!(m.n_items(), 3);
        let sec = m.item_link(0, gio::MENU_LINK_SECTION).expect("section 0");
        assert_eq!(sec.n_items(), 2);
        assert_eq!(attr(&sec, "label", 0).as_deref(), Some("New Workbench"));
        assert_eq!(attr(&sec, "action", 0).as_deref(), Some("app.new-workbench"));
        assert_eq!(attr(&sec, "label", 1).as_deref(), Some("Open Workbench"));
        assert_eq!(attr(&sec, "action", 1).as_deref(), Some("app.open-workbench"));
        let sec = m.item_link(1, gio::MENU_LINK_SECTION).expect("section 1");
        assert_eq!(sec.n_items(), 1);
        assert_eq!(attr(&sec, "label", 0).as_deref(), Some("Save Workbench"));
        assert_eq!(attr(&sec, "action", 0).as_deref(), Some("app.save-workbench"));
        let sec = m.item_link(2, gio::MENU_LINK_SECTION).expect("section 2");
        assert_eq!(sec.n_items(), 1);
        assert_eq!(attr(&sec, "label", 0).as_deref(), Some("Exit"));
        assert_eq!(attr(&sec, "action", 0).as_deref(), Some("app.quit"));
    }
}
