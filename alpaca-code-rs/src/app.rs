//! App component — the window shell: headerbar
//! (File/Edit/Window/Help label buttons + run/stop cells), hpaned → vpaned
//! (editor / panes placeholder cards) + file-browser card, workspace lifecycle
//! (set_workspace / discard flow), menus/actions, and the persist-tabs
//! debounce. Slot cards stay in the model; Tasks 6–8 pack real components.

// 4.10-deprecated Dialog/FileChooser/MessageDialog API — the new-project and
// open-file dialogs ride the deprecated Gtk flow; S3 polish modernizes.
#![allow(deprecated)]

use std::cell::Cell;
use std::path::{Path, PathBuf};

use relm4::gtk::{self, gdk, gio, glib, pango, prelude::*};
use relm4::component::ComponentController;
use relm4::{Component, ComponentParts, ComponentSender};

use crate::editor::{Editor, EditorMsg, EditorOutput};
use crate::gitpanel::{GitPanel, GitPanelMsg, GitPanelOutput, ProbeChild};
use crate::gitview::GitViewMsg;
use crate::panes::{Panes, PanesMsg, PanesOutput};
use crate::{runctl, state, vector};

pub use crate::editor::EditorSnapshot;

#[derive(Debug)]
pub enum AppMsg {
    /// every open path (File▸Open Project…, recents, "+", New Project…,
    /// probe chains): switch if open / add under the cap / report when full
    OpenWorkspace(PathBuf),
    /// header-pill click → instant switch (requirement 8)
    SwitchWorkspace(usize),
    /// startup: swing to the remembered project after ALL queued opens ran
    BootActivate(PathBuf),
    /// requirement 3: header "+" → the folder picker (shared open path)
    AddProjectDialog,
    /// tree → editor routing within the tagged workspace
    OpenFile(usize, PathBuf),
    /// editor state flip → snapshot storage + persist debounce
    EditorState(usize, EditorSnapshot),
    /// panes → header Run/Stop sync (launch accepted — the starting window)
    RunStarting(usize),
    RunStatus(usize, bool),
    /// the run/stop pills (shared chrome → the ACTIVE workspace's panes)
    RunPressed,
    StopPressed,
    FlushTabs,
    /// open-recent target no longer exists → info dialog
    RecentMissing(String),
    /// CHANGES row → editor diff page (tagged)
    OpenDiff {
        ws: usize,
        rel: String,
        letter: String,
    },
    /// gitview commit button → that workspace's flush chain
    Commit {
        ws: usize,
        root: PathBuf,
        paths: Vec<String>,
        msg: String,
    },
    /// editor SaveOpen's ack — the ONLY thing that releases pending_commit
    Flushed(usize),
    /// git rows → that workspace's diff-tab renewal
    GitChanged(usize, crate::gitstatus::Rows),
    /// window notify::is-active → the ACTIVE workspace's git re-sync
    WindowActive,
    /// Edit menu → the active workspace's editor
    EditOp(EditKind),
    /// New Project dialog result — makedirs + `git init` ran OFF the main
    /// thread (runctl.create_project has a 10s wall; a stalled git must not
    /// freeze the window). The Sender marshals back onto the main context.
    ProjectCreated { target: PathBuf, res: Result<(), String> },
    /// PROBE-ONLY (ALPACA_PROBE_S2) — never runs in production (env unset)
    ProbeS2(PathBuf),
    /// PROBE-ONLY (ALPACA_PROBE_S2) — the step-chain driver
    ProbeS2Step,
    /// PROBE-ONLY (ALPACA_PROBE_S3) — never runs in production (env unset)
    ProbeS3(PathBuf),
    /// PROBE-ONLY (ALPACA_PROBE_S3) — the step-chain driver
    ProbeS3Step,
    /// PROBE-ONLY (ALPACA_PROBE_S3) — 60ms damage keeper (probe script kills
    /// the process right after DONE, so its forever timer is harmless)
    ProbeDraw,
    /// PROBE-ONLY (ALPACA_PROBE_S4) — never runs in production (env unset)
    ProbeS4(PathBuf),
    /// PROBE-ONLY (ALPACA_PROBE_S4) — the step-chain driver
    ProbeS4Step,
}

thread_local! {
    /// PROBE-ONLY (ALPACA_PROBE_S3) keeper tick counter for the width traces
    static PROBE_S3_TICKS: Cell<u32> = const { Cell::new(0) };
}

/// Requirement 5 — maximum simultaneously open workspaces.
pub const MAX_WORKSPACES: usize = 3;

/// open_workspace's routing decision (pure; the unit test pins it).
#[derive(Debug, PartialEq, Eq)]
pub enum AddDecision {
    /// already open → switch to this strip index (no re-root, no restore)
    Switch(usize),
    /// not open, under the cap → add to the right
    Add,
    /// cap reached (requirement 5) — the caller reports, nothing opens
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

/// Edit-menu ops route by active workspace (the actions now fan out to N
/// editors; the shared actions can't hold a Sender per editor).
#[derive(Debug, Clone, Copy)]
pub enum EditKind {
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
}

/// One open project: its own editor card, VTE pane card, git panel card and
/// header-strip pill. Switching = a GtkStack visibility flip over these
/// assemblies — everything (VTE children, git probe timers, editor pages,
/// scroll pairing) stays ALIVE while hidden, which is the feature: a
/// project's processes keep running in the background. No re-rooting ever.
pub struct Workspace {
    root: PathBuf,
    /// the card assembly (hpaned → [editor | panes-vpaned, gitpanel]) — one GtkStack page
    hpane: gtk::Paned,
    /// the header-strip pill (App places it left of nothing; T3 adds "+" after)
    btn: gtk::Button,
    editor: relm4::Controller<Editor>,
    panes: relm4::Controller<Panes>,
    gitpanel: relm4::Controller<GitPanel>,
    /// the running mirror for the shared header Run/Stop pills
    running: bool,
    /// latest editor snapshot — FlushTabs persists EVERY workspace's
    snapshot: EditorSnapshot,
    /// gitview Commit → App → SaveOpen → Flushed(ack) → gitpanel StartCommit,
    /// per workspace so a switch mid-flight can't strand the chain
    pending_commit: Option<(PathBuf, Vec<String>, String)>,
}

impl Workspace {
    /// Build the whole per-project UI. `ws` = the index this instance will
    /// occupy in App's vec; component outputs are tagged with it so App
    /// routes back into THIS workspace regardless of the active one.
    fn new(path: PathBuf, ws: usize, app_out: ComponentSender<App>) -> Self {
        let path_str = path.to_string_lossy().into_owned();

        // Task 6 (editor card) — the component carries its own alpaca-card
        let editor_slot = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let app_in = app_out.clone();
        let editor = Editor::builder()
            .launch(())
            .connect_receiver(move |_, out| match out {
                EditorOutput::StateChanged(s) => app_in.input(AppMsg::EditorState(ws, s)),
                EditorOutput::Flushed => app_in.input(AppMsg::Flushed(ws)), // the flush chain's ack
            });
        let tabs = state::project_tabs(&state::load(), &path_str);
        editor_slot.append(editor.widget());

        // Task 8 (the real panes card)
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
        // design: editor card 56..649 (593) − pane handle overlap
        vpane.set_position(592);

        // git panel card (the hpane end child)
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

        // hpane DIRECTLY as the stack page child — a wrapping Box under-allocates
        // (measured 43px tall)
        let hpane = gtk::Paned::new(gtk::Orientation::Horizontal);
        hpane.set_wide_handle(true);
        hpane.set_start_child(Some(&vpane));
        hpane.set_end_child(Some(gitpanel.widget()));
        // design: cards gap columns 1172..1180 → pane pos = card end − 10.
        // NOT re-applied at switch (the position property must survive the
        // hidden page's first allocation — the S4 probe asserts exactly that).
        hpane.set_position(1161);

        // Requirement 2: the pill's idle look IS the old title look
        // (.alpaca-projbtn mirrors .alpaca-wintitle); hover = the branch pill's.
        let btn = gtk::Button::builder()
            .has_frame(false)
            .css_classes(["alpaca-projbtn"])
            .build();
        let lbl = gtk::Label::builder().label(py_basename(&path_str)).build();
        lbl.set_ellipsize(pango::EllipsizeMode::End);
        lbl.set_max_width_chars(24); // 3 long names must not blow the strip out
        btn.set_child(Some(&lbl));
        btn.set_tooltip_text(Some(&path_str));
        // requirement 6: every pill is clickable → instant switch
        let snd = app_out.clone();
        btn.connect_clicked(move |_b| snd.input(AppMsg::SwitchWorkspace(ws)));

        // tree first, then panes, editor Restore LAST (the old set_workspace
        // order — restore must ride after the re-roots it reads back)
        gitpanel.emit(GitPanelMsg::SetRoot(path.clone()));
        panes.emit(PanesMsg::SetRoot(path.clone()));
        editor.emit(EditorMsg::Restore(
            path.clone(),
            tabs.open_tabs.clone(),
            tabs.active_tab,
        ));

        Self {
            root: path,
            hpane,
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
    run_btn: gtk::Button,
    stop_btn: gtk::Button,
    run_row: gtk::Box,
    ws: Vec<Workspace>,
    /// valid iff !ws.is_empty() — never read on empty
    active: usize,
    /// PROBE-ONLY (ALPACA_PROBE_S2) — step chain (workspace, step index);
    /// production never constructs it (env unset)
    probe_ws: Option<(PathBuf, usize)>,
    /// PROBE-ONLY (ALPACA_PROBE_S3) — step chain (workspace, step index);
    /// production never constructs it (env unset)
    probe_s3: Option<(PathBuf, usize)>,
    /// PROBE-ONLY (ALPACA_PROBE_S4) — step chain (workspace, step index);
    /// production never constructs it (env unset)
    probe_s4: Option<(PathBuf, usize)>,
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
        let arg = model.clone(); // the CLI arg — read before `model` is shadowed

        // Mockup's pill buttons: green play / red stop — design SVG (theme
        // symbols grey both out); t4's symbolic stand-in paid off — badges (t5)
        // carries the art. valign CENTER: default FILL lands ink top-left
        // (measured 7px high).
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
        // Requirements 1/6/7: the title label is GONE — the header center is a
        // strip of project pills, centered as ONE entity by the headerbar's own
        // title-widget centering. 6px tasteful gaps (requirement 7).
        let proj_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        // the strip must NOT fill the bar: default FILL stretches every pill
        // band to the header's 32px interior (image #8: band flush top/bottom).
        // CENTER pins band = the strip's own req with air each side — the
        // run_row recipe (band = the row it stands in, app.rs:396-398) applied
        // at strip level.
        proj_box.set_valign(gtk::Align::Center);
        // requirement 3: "+" to the right of the project pills; the strip is
        // [pill … pill | +] — the same flat pill family
        // the "+" rides plus.svg's stroke art: a text "+" is an x-height glyph
        // (measured image #9, ~7px vs the pill text's 9px cap band) and carried
        // no hover band. Same recipe as the run icons (paintable + pixel_size +
        // valign); 14px matches the 13px pill text; the baked #8a93a6 stroke
        // matches the branch-icon family (branch.svg stays grey on hover too —
        // baked-stroke law: recolor is opacity/alt-art only, CSS color can't
        // reach a Gsk node).
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
            // art loss (never on a shipped build): the old text stand-in
            add_btn.set_child(Some(&gtk::Label::builder().label("+").build()));
        }
        add_btn.set_tooltip_text(Some("Open project…"));
        proj_box.append(&add_btn);
        let send = sender.clone();
        add_btn.connect_clicked(move |_b| send.input(AppMsg::AddProjectDialog));
        header.set_title_widget(Some(&proj_box));
        // The one header menu: a small down arrow (chevron-down.svg — 16-grid,
        // w2 #8A93A6, the branch-art family) opening the workbench actions.
        // Custom-child = icon mode: MenuButton hides its own arrow (measured
        // law), so no arrow walk. Header menubuttons are styled by the bare
        // `.alpaca-header menubutton > button` css rule. img valign CENTER rides
        // the run-icon law (default FILL parks ink high in the taller button).
        let menu_btn = gtk::MenuButton::new();
        if let Some(vec) = vector::icon("chevron-down.svg") {
            let img = gtk::Image::from_paintable(Some(&vec));
            img.set_pixel_size(14);
            img.set_valign(gtk::Align::Center);
            menu_btn.set_child(Some(&img));
        } else {
            // art loss (never on a shipped build): symbolic stand-in
            menu_btn.set_child(Some(&gtk::Image::builder()
                .icon_name("pan-down-symbolic")
                .pixel_size(14)
                .valign(gtk::Align::Center)
                .build()));
        }
        menu_btn.set_tooltip_text(Some("Workbench"));
        menu_btn.set_popover(Some(&gtk::PopoverMenu::from_model(Some(&workbench_menu()))));
        header.pack_start(&menu_btn);

        // Hover swatches live on wrapper cells, not the buttons: a GtkButton's
        // CSS box paints 16+2·pad-tall around a 16px icon while its measure adds
        // a constant ~18px phantom. GtkBox cells paint their FULL allocation
        // (fill), so the cells carry :hover and the buttons stay flat forever.
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
        run_row.insert_child_after(&divider, Some(&first)); // run | div | stop
        // band = 24 exactly BECAUSE the row stands 24 tall (CENTER in the 32px
        // bar = 4px air each side — equalized 2026-10-14 with the project
        // pills' selected band and the menubutton margin, measured image #13);
        // header interior would FILL it to 32.
        run_row.set_valign(gtk::Align::Center);
        header.pack_end(&run_row);
        root.set_titlebar(Some(&header));

        // run/stop pills (button clicks route through the model)
        let send = sender.clone();
        run_btn.connect_clicked(move |_b| send.input(AppMsg::RunPressed));
        let send = sender.clone();
        stop_btn.connect_clicked(move |_b| send.input(AppMsg::StopPressed));

        // clamped AFTER construction, same numbers
        let widgets = view_output!();
        let (w, h) = default_size();
        root.set_default_size(w, h);

        // The window's content = one stack page per open workspace
        // (requirement 8: transition None = instant, like browser tabs).
        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::None)
            .hexpand(true)
            .vexpand(true)
            .build();
        root.set_child(Some(&stack));

        let model = App {
            stack,
            proj_box,
            add_btn,
            run_btn,
            stop_btn,
            run_row,
            ws: Vec::new(),
            active: 0,
            probe_ws: None,
            probe_s3: None,
            probe_s4: None,
        };

        register_actions(&relm4::main_application(), &root, sender.clone());

        // notify::is-active fires on EVERY notify (both False→True and the rare
        // True→False edge); each fire costs one sync refresh — keep exactly that.
        let in_active = sender.clone();
        root.connect_notify_local(Some("is-active"), move |_, _| {
            in_active.input(AppMsg::WindowActive)
        });

        // startup: reopen last session's strip (open_projects; old files
        // coerce to [last_project] via the fallback), then swing active to the
        // remembered one. BootActivate must ride the SAME queue as the opens.
        let st = state::load();
        let mut opens: Vec<PathBuf> = state::open_projects(&st)
            .iter()
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
            .collect();
        if opens.is_empty()
            && let Some(l) = st.last_project.clone()
            && Path::new(&l).is_dir()
        {
            opens.push(PathBuf::from(l));
        }
        for p in opens {
            sender.input(AppMsg::OpenWorkspace(p));
        }
        if let Some(l) = st.last_project {
            sender.input(AppMsg::BootActivate(PathBuf::from(l)));
        }
        // the CLI arg opens LAST — it ends active (the old launch's whole point)
        if let Some(p) = arg {
            sender.input(AppMsg::OpenWorkspace(p));
        }
        // PROBE-ONLY (ALPACA_PROBE_S2) — never runs in production (env unset)
        if let Ok(dir) = std::env::var("ALPACA_PROBE_S2") {
            sender.input(AppMsg::ProbeS2(PathBuf::from(dir)));
        }
        // PROBE-ONLY (ALPACA_PROBE_S3) — never runs in production (env unset)
        if let Ok(dir) = std::env::var("ALPACA_PROBE_S3") {
            sender.input(AppMsg::ProbeS3(PathBuf::from(dir)));
        }
        // PROBE-ONLY (ALPACA_PROBE_S4) — never runs in production (env unset)
        if let Ok(dir) = std::env::var("ALPACA_PROBE_S4") {
            sender.input(AppMsg::ProbeS4(PathBuf::from(dir)));
        }

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match msg {
            AppMsg::OpenWorkspace(p) => self.open_workspace(&p, root, sender),
            AppMsg::SwitchWorkspace(i) => self.switch_to(i, root),
            AppMsg::BootActivate(p) => {
                if let Some(i) = self.ws.iter().position(|w| w.root.as_path() == p.as_path()) {
                    self.switch_to(i, root);
                }
            }
            AppMsg::AddProjectDialog => open_project(root, sender),
            AppMsg::OpenFile(ws, p) => {
                // filetree → editor routing within the tagged workspace
                if let Some(w) = self.ws.get_mut(ws) {
                    w.editor.emit(EditorMsg::OpenFile(p));
                }
            }
            AppMsg::EditorState(ws, s) => {
                if let Some(w) = self.ws.get_mut(ws) {
                    w.snapshot = s;
                }
                if !self.ws.is_empty() {
                    // debounced ≤1s; each event re-arms (stacked timers flush
                    // the LATEST snapshots of EVERY workspace; last event
                    // before quit may be lost — acceptable, as before)
                    let sender = sender.clone();
                    glib::timeout_add(std::time::Duration::from_millis(1000), move || {
                        sender.input(AppMsg::FlushTabs);
                        glib::ControlFlow::Break
                    });
                }
            }
            AppMsg::FlushTabs => {
                let mut st = state::load();
                for w in &self.ws {
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
                // the run-status payload is not displayed; the
                // buttons + pill class follow the workspace's running mirror
                if let Some(w) = self.ws.get_mut(ws) {
                    w.running = running;
                }
                if ws == self.active {
                    self.run_btn.set_sensitive(!running);
                    self.stop_btn.set_sensitive(running);
                    self.refresh_run_style();
                }
            }
            AppMsg::RunStarting(ws) => {
                if let Some(w) = self.ws.get_mut(ws) {
                    w.running = true;
                }
                if ws == self.active {
                    self.run_btn.set_sensitive(false); // Stop-only until run ends
                    self.refresh_run_style();
                }
            }
            AppMsg::RunPressed => {
                // no workspace yet / live run → noop
                let running = self.ws.get(self.active).map(|w| w.running).unwrap_or(true);
                if running {
                    return;
                }
                let cmd = self
                    .ws
                    .get(self.active)
                    .and_then(|w| runctl::detect(w.root.as_path()));
                let Some(cmd) = cmd else {
                    self.run_btn.set_sensitive(false);
                    self.run_btn
                        .set_tooltip_text(Some("No dev/start script or .csproj found"));
                    return;
                };
                self.stop_btn.set_sensitive(true);
                if let Some(w) = self.ws.get(self.active) {
                    w.panes.emit(PanesMsg::LaunchRun { argv: cmd.argv });
                }
                // run-off + style land with RunStarting (accepted) —
                // one mainloop turn, same frame
            }
            AppMsg::StopPressed => {
                // unconditional button sync, then the active workspace's panes
                if let Some(w) = self.ws.get(self.active) {
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
                // the diff open runs within the tagged workspace
                if let Some(w) = self.ws.get(ws) {
                    w.editor.emit(EditorMsg::OpenDiff { root: w.root.clone(), rel, letter });
                }
            }
            AppMsg::Commit { ws, root, paths, msg } => {
                // the commit's dirty-file flush (editor.save_open) must run
                // SYNCHRONOUSLY on the commit's mainloop turn — relm4 inputs
                // are QUEUE-processed, not call-stack: the turn's equivalent
                // is the workspace's pending store + the Flushed ack chain.
                // SaveOpen FIRST (paths by ref), the store move second —
                // pending_commit takes ownership of the payload tail
                if let Some(w) = self.ws.get_mut(ws) {
                    w.editor.emit(EditorMsg::SaveOpen(
                        paths.iter().map(|rel| root.join(rel)).collect(),
                    ));
                    w.pending_commit = Some((root, paths, msg));
                }
            }
            AppMsg::Flushed(ws) => {
                // the editor's Flushed strictly follows its SaveOpen in its
                // FIFO input queue → a sound chain (T5's_save_open ALWAYS
                // emits Flushed, zero-flushed included — the chain cannot
                // strand; eager-busy closes the duplicate-click window)
                if let Some(w) = self.ws.get_mut(ws)
                    && let Some((root, paths, msg)) = w.pending_commit.take() {
                        w.gitpanel.emit(GitPanelMsg::StartCommit { root, paths, msg });
                    }
            }
            AppMsg::GitChanged(ws, rows) => {
                // diff-tab renewal for the tagged workspace (a stale diff tab
                // across WORKSPACE switches does not exist anymore — each
                // workspace's editor carries its own diffs; restore rebuilds
                // ONLY path pages for the boot path)
                if let Some(w) = self.ws.get(ws) {
                    let letters = rows.iter().map(|(rel, letter)| (rel.clone(), letter.clone())).collect();
                    w.editor.emit(EditorMsg::RefreshDiffs { root: w.root.clone(), letters });
                }
            }
            AppMsg::WindowActive => {
                // window focus re-syncs the ACTIVE workspace's git
                if let Some(w) = self.ws.get(self.active) {
                    w.gitpanel.emit(GitPanelMsg::RefreshGit);
                }
            }
            AppMsg::EditOp(kind) => {
                if let Some(w) = self.ws.get_mut(self.active) {
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
            AppMsg::ProbeS2(dir) => {
                // PROBE-ONLY (ALPACA_PROBE_S2) — never runs in production
                // (env unset); step chain start: schedule step 0 with the
                // table's +300ms lead
                self.probe_ws = Some((dir, 0));
                let snd = sender.clone();
                glib::timeout_add(std::time::Duration::from_millis(PROBE_STEP_MS[0]), move || {
                    snd.input(AppMsg::ProbeS2Step);
                    glib::ControlFlow::Break
                });
            }
            AppMsg::ProbeS2Step => self.probe_step(root, &sender),
            AppMsg::ProbeDraw => {
                // PROBE-ONLY (ALPACA_PROBE_S3) — an occluded Wayland surface
                // gets NO compositor frame callbacks, so GTK allocations
                // freeze at their startup values after the first paint
                // (measured: hpane.set_position left allocs stale for 2s+).
                // 60ms damage keeps the surface's frame clock ticking through
                // the whole chain; the timer is never disarmed but the probe
                // process is killed right after PROBE S3 DONE.
                if self.probe_s3.is_some() {
                    root.queue_draw();
                    PROBE_S3_TICKS.with(|t| {
                        let n = t.get() + 1;
                        t.set(n);
                        // ~every 8th tick (480ms): are allocations moving?
                        if n % 8 == 0 {
                            println!("PROBE tick {n}: root w={}", root.allocated_width());
                        }
                    });
                }
            }
            AppMsg::ProbeS4(dir) => {
                // PROBE-ONLY (ALPACA_PROBE_S4) — never runs in production
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
                        say("two-open", self.ws.len() == 2);
                        // THE feature asserts, live: state survives the flip.
                        // GObject PartialEq compares memory addresses
                        // (verified in glib-0.22 object.rs) — Widget vs Paned
                        // is the same instance's pointer, no upcast dance.
                        say(
                            "stack-visible-is-wsA",
                            self.stack
                                .visible_child()
                                .is_some_and(|v| v == self.ws[0].hpane),
                        );
                        say(
                            "editor-tabs-preserved",
                            // snapshot open_tabs are RELPATHS (editor emit_state)
                            self.ws[0].snapshot.open_tabs == vec!["a.txt".to_string()],
                        );
                        // the paned's first-allocation law: the position clamps
                        // to max = allocated_width − 6px separator — a narrow
                        // virtual display (broadway vfb) reads BELOW 1161 by
                        // exactly that clamp; assert the constructed position
                        // survived (no recompute/collapse), clamp included
                        let hw = self.ws[0].hpane.allocated_width();
                        let pos = self.ws[0].hpane.position();
                        println!("PROBE hpane-pos reads pos={pos} hpane-w={hw}");
                        // hw==0 → never allocated yet by assert time (racy on
                        // the frame clock): the property must read 1161.
                        // allocated (narrow vfb) → the clamp law applies.
                        let ok = if hw > 0 { pos == 1161.min(hw - 6) } else { pos == 1161 };
                        say("hpane-pos-1161", ok);
                        say(
                            "on-class-wsA",
                            self.ws[0].btn.has_css_class("alpaca-on")
                                && !self.ws[1].btn.has_css_class("alpaca-on"),
                        );
                        // polish task: the pill band must FLOAT —
                        // stretched-to-the-bar is the image-#8 regression
                        // (default FILL); Center pins band = the strip's own
                        // req. Uniformity is free: all strip children share
                        // the strip's height. A bh of 0 = never-allocated
                        // (the S4 hpane comment's race) → inconclusive,
                        // rerun; never tune CSS off a 0.
                        let bh = self.proj_box.allocated_height();
                        let bands: Vec<i32> =
                            self.ws.iter().map(|w| w.btn.allocated_height()).collect();
                        println!("PROBE strip: proj_box-h={} pill-band-heights={:?}", bh, bands);
                        let ok = bh > 0
                            && bands.len() == 2
                            && bands.iter().all(|h| *h == bh && *h < 32);
                        say("pill-band-floats", ok);
                        // polish task: the "+" lost its text label for the
                        // plus.svg stroke icon (image #9: a text "+" is an
                        // x-height glyph, ~7px next to 9px cap text)
                        say(
                            "add-btn-icon",
                            self.add_btn.child().and_downcast::<gtk::Image>().is_some(),
                        );
                        // requirement 1: the WM title carries the bare name
                        say(
                            "title-is-basename",
                            root.title().as_deref()
                                == Some(self.ws[0].root.file_name().unwrap().to_str().unwrap()),
                        );
                    }
                    3 => {
                        let ws_c = ws_dir.to_string_lossy().replace("wsA", "wsC");
                        self.open_workspace(std::path::Path::new(&ws_c), root, sender.clone());
                        say("three-open", self.ws.len() == 3);
                        say("add-btn-disabled", !self.add_btn.is_sensitive());
                        // state.json's open set must mirror the LIVE strip:
                        // the rig seeds a ghost dir (GONE, deleted) — boot
                        // filters it from the strip, and this ADD must prune
                        // it from the open set (a persisted ghost steals a
                        // cap slot and evicts live workspaces at next boot)
                        sender.spawn_command(move |_| {
                            let st = std::fs::read_to_string(crate::state::state_path())
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
                        // active is wsC now; swing back to wsA via the real
                        // switch path
                        self.switch_to(0, root);
                    }
                    // requirement: an already-open path must SWITCH (the
                    // add_decision dedupe), never duplicate the strip pill
                    4 => {
                        self.open_workspace(ws_dir.as_path(), root, sender.clone());
                        say("re-open-wsA-switches", self.ws.len() == 3 && self.active == 0);
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
            AppMsg::ProbeS3(dir) => {
                // PROBE-ONLY (ALPACA_PROBE_S3) — never runs in production
                // (env unset); step chain start: schedule step 0 with the
                // table's first gap
                self.probe_s3 = Some((dir, 0));
                // force map+activate: an occluded surface's frame clock stalls
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
                // PROBE-ONLY (ALPACA_PROBE_S3) — never runs in production
                // (env unset); one chain, each timer BREAKs — no re-arm hazard;
                // emissions target the ACTIVE workspace (the chain's own open)
                let Some((ws_dir, step)) = self.probe_s3.as_ref().map(|(w, s)| (w.clone(), *s)) else {
                    return;
                };
                println!("PROBE step {}: ts={}", step, epoch_ms());
                // the S3 split targets: the ACTIVE workspace's hpane (the old
                // window-children walk is dead under the stack — the paned
                // lives inside the active stack page now)
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
                        let Some(w) = self.ws.get(self.active) else { return };
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
                        // XWayland path: KWin need not honor default_size and
                        // 900/850 must straddle the legal range (hpane 677 at
                        // the wide-trace = both rounds clamp identically) —
                        // floor the window BEFORE the wide split (a same-step
                        // floor+set_position garbles the paned position:
                        // readback came back 1568, the split never applied)
                        root.set_size_request(1586, 992);
                    }
                    3 => if let Some(w) = self.ws.get(self.active) { w.hpane.set_position(900); },
                    4 => {
                        let Some(w) = self.ws.get(self.active) else { return };
                        println!("PROBE hpane pos={} w={} at wide-trace",
                                 w.hpane.position(), w.hpane.allocated_width());
                        w.editor.emit(EditorMsg::ProbeTrace("wide".into()));
                    }
                    5 => if let Some(w) = self.ws.get(self.active) { w.hpane.set_position(850); },
                    6 => {
                        if let Some(w) = self.ws.get(self.active) {
                            w.editor.emit(EditorMsg::ProbeTrace("narrow".into()));
                        }
                        println!("PROBE S3 DONE");
                        self.probe_s3 = None;
                        return;
                    }
                    _ => {} // unreachable — the chain drives 0..=6 only
                }
                // S2 law carried: driver re-arms by incrementing the pair in place
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

/// PROBE-ONLY (ALPACA_PROBE_S2) step table — gap[i] = ms delay between step
/// i-1 and step i (the schedule arms step_ms(step+1) after each step).
/// Production never runs the chain (env guard at init).
///
/// 0 SetRoot wsA · 1 ModeChanges · 2 ProbeMsg · 3 OpenDiff a.txt ·
/// 4 ProbeClip (fix-3; both run PRE-SWITCH — SetWorkspace closes every
/// page) · 5-8 search-entry keystrokes on wsA in CHANGES mode (the typing
/// deltas are the fix-2 assert: a per-keystroke `git status` stall
/// straddles the next step's timer and inflates the gap; the shim sleeps
/// 1.0s on status) · 9 clear the needle · 10 masthead ×2 · 11 CommitClicked
/// · 12 SetWorkspace wsB · 13 fixture asserts · 14 terminate (its dwell
/// must cover TWO post-switch 2s probe ticks — assert A needs both).
const PROBE_STEP_MS: [u64; 15] = [
    300, 500, 450, 350, 400, 400, 400, 400, 400, 400, 400, 300, 300, 800, 2800,
];

// PROBE-ONLY (ALPACA_PROBE_S3) — step chain: see steps table on this line.
// 0 = SetWorkspace + 6 OpenFile (batch; widths read ~1.4s later, mapped)
// 1 = static-min asserts (window ≤560 editor ≤300 panes ≤300 + TreeMin ≤170)
// 2 = window size floor 1586x992 (XWayland: KWin need not honor default_size)
// 3 = hpane.set_position(900)
// 4 = editor ProbeTrace("wide")        ← taballoc wide print
// 5 = hpane.set_position(850)
// 6 = editor ProbeTrace("narrow") + "PROBE S3 DONE"
const PROBE_S3_STEP_MS: [u64; 7] = [500, 700, 700, 700, 600, 600, 600];

// PROBE-ONLY (ALPACA_PROBE_S4) — step chain: multi-workspace asserts; wsB/wsC
// are the passed dir's name-replaced siblings.
// 0 = open wsA (+ open a.txt) · 1 = open wsB (+ open b.txt) ·
// 2 = switch BACK to wsA + live asserts (stack identity, tabs, paned pos,
//     pill classes, WM title) · 3 = open wsC → cap-full asserts
//     (3 open, "+" disabled) · 4 = re-open wsA → must SWITCH not add → DONE
const PROBE_S4_STEP_MS: [u64; 6] = [500, 700, 700, 700, 700, 500];

/// epoch ms for the probe's step trace (the wrapper correlates app ts with
/// the git-shim log's ts)
pub(crate) fn epoch_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// gap for the schedule-after-step — .get() so a stray fire can't panic
fn step_ms(i: usize) -> u64 {
    PROBE_STEP_MS.get(i).copied().unwrap_or(0)
}

impl App {
    /// drives the step at `probe_ws`'s index, then schedules the next one;
    /// emissions target the ACTIVE workspace (the chain's own opens).
    fn probe_step(&mut self, root: &gtk::ApplicationWindow, sender: &ComponentSender<Self>) {
        // gap[i] — see the table const above
        const PROBE_MSG: &str = "s2 probe commit";
        let Some((ws, step)) = self.probe_ws.as_ref().map(|(w, s)| (w.clone(), *s)) else {
            return;
        };
        // probe convention: the wrapper lays the sibling repo beside wsA
        let ws_b = ws.to_string_lossy().replace("wsA", "wsB");
        // probe-only step trace, epoch ms — the wrapper times asserts against it
        println!("PROBE step {}: ts={}", step, epoch_ms());
        match step {
            // the REAL workspace route — panel SetRoot + editor Restore + the
            // open set; the OpenDiff arm needs this workspace active
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
                if let Some(w) = self.ws.get(self.active) {
                    w.gitpanel.emit(GitPanelMsg::ModeChanges);
                }
            }
            // commit_clicked rejects an empty entry — fill the real entry
            2 => {
                if let Some(w) = self.ws.get(self.active) {
                    w.gitpanel.emit(GitPanelMsg::ProbeChild(ProbeChild::Changes(
                        GitViewMsg::ProbeMsg(PROBE_MSG.into()),
                    )));
                }
            }
            // the diff page for the fix-3 assert (Edit-menu target on a diff
            // page must be the focused side view)
            3 => sender.input(AppMsg::OpenDiff {
                ws: self.active,
                rel: "a.txt".into(),
                letter: "M".into(),
            }),
            4 => {
                if let Some(w) = self.ws.get(self.active) {
                    w.editor.emit(EditorMsg::ProbeClip);
                }
            }
            // typing steps — fill the real search entry; GtkSearchEntry's
            // delayed "search-changed" fires the REAL EntryChanged route.
            // Pre-fix the changes arm re-fetches (SYNC git per keystroke);
            // its stall straddles the next typing step's timer — deltas ≈
            // 150ms delay + shim sleep (green ≈ the 400ms gap).
            5 => {
                if let Some(w) = self.ws.get(self.active) {
                    w.gitpanel.emit(GitPanelMsg::ProbeEntry("alpha".into()));
                }
            }
            6 => {
                if let Some(w) = self.ws.get(self.active) {
                    w.gitpanel.emit(GitPanelMsg::ProbeEntry("alphabet".into()));
                }
            }
            7 => {
                if let Some(w) = self.ws.get(self.active) {
                    w.gitpanel.emit(GitPanelMsg::ProbeEntry("alphabetZ".into()));
                }
            }
            8 => {
                if let Some(w) = self.ws.get(self.active) {
                    w.gitpanel.emit(GitPanelMsg::ProbeEntry("alphabetZx".into()));
                }
            }
            // clear the needle so the masthead sees the full set again;
            // ProbeDiffScroll rides the same PRE-SWITCH step (the diff page is
            // ~2s old here and settled — step 4 timing is racy, measured;
            // step 12's open closes nothing — it ADDS wsB, so the pre-switch
            // window is the settled slot)
            9 => {
                if let Some(w) = self.ws.get(self.active) {
                    w.gitpanel.emit(GitPanelMsg::ProbeEntry("".into()));
                    w.editor.emit(EditorMsg::ProbeDiffScroll);
                }
            }
            // select-all via the REAL row-toggle law — but rows arrive
            // PRE-CHECKED (a fresh view selects all, gitview SetRoot
            // → refresh(false)): one masthead toggle would DESELECT. Toggle
            // twice (off→on) so the live toggle path runs and the state ends
            // all-checked.
            10 => {
                if let Some(w) = self.ws.get(self.active) {
                    let mast = || GitPanelMsg::ProbeChild(ProbeChild::Changes(
                        GitViewMsg::RowToggled(gtk::TreePath::from_string("0").unwrap()),
                    ));
                    w.gitpanel.emit(mast()); // off
                    w.gitpanel.emit(mast()); // on — ends all-checked
                }
            }
            // the FULL production chain: validate → eager busy → Commit out
            // → App Commit → SaveOpen → Flushed → panel StartCommit → flight
            11 => {
                if let Some(w) = self.ws.get(self.active) {
                    w.gitpanel.emit(GitPanelMsg::ProbeChild(ProbeChild::Changes(
                        GitViewMsg::CommitClicked,
                    )));
                }
            }
            // MID-FLIGHT injection: wsB is its own clean repo; under the
            // multi-workspace semantics this ADDS wsB as workspace 2 and
            // switches (the flight's root was SNAPSHOT into StartCommit's
            // payload, so the commit still lands on wsA — asserted next)
            12 => self.open_workspace(Path::new(&ws_b), root, sender.clone()),
            // assertions — a spawn_command runs the fixture asserts off-chain
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
                    // 2 = the fixture's base commit + this probe's own commit
                    // (the brief's ==1 counted only the probe commit — with a
                    // base commit in the fixture the total is deterministic 2)
                    say(
                        "wsA-commit-count",
                        git(&ws_a, &["rev-list", "--count", "HEAD"]) == "2",
                    );
                    say(
                        "wsA-commit-msg",
                        git(&ws_a, &["log", "-1", "--format=%s"]) == PROBE_MSG,
                    );
                    say("wsB-commit-count", git(&wsb, &["rev-list", "--count", "--all"]) == "0");
                    // pushed = origin/main == HEAD (count is fixture-dependent:
                    // base + probe = 2)
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
                    // zero-flush proof = wsA-commit-count PASS (T5's
                    // Flushed-always ack carried the chain over zero pages)
                });
            }
            14 => {
                println!("PROBE S2 DONE");
                self.probe_ws = None;
                return;
            }
            _ => return,
        }
        // PROBE-ONLY capture gate (ALPACA_PROBE_S2_HOLD=N): run steps
        // 0..=N-1, then park the app with the diff page open — the full
        // chain's tail closes the page (SetWorkspace wsB), so a staged pixel
        // capture cannot use it. Production never reads this (env unset).
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

    /// Every workspace-open path lands here (header "+", File ▸ Open Project…,
    /// recents, New Project…, probe chains): switch if open, add to the right
    /// under the cap, else report (requirements 3/4/5). Nothing is ever
    /// destroyed on open, so the old discard dialog is GONE — unsaved buffers
    /// survive inside their own always-alive workspace.
    fn open_workspace(
        &mut self,
        path: &Path,
        root_win: &gtk::ApplicationWindow,
        sender: ComponentSender<Self>,
    ) {
        let open: Vec<PathBuf> = self.ws.iter().map(|w| w.root.clone()).collect();
        match add_decision(&open, path) {
            AddDecision::Full => {
                error_dialog(root_win, "Maximum of 3 projects open", "");
            }
            AddDecision::Switch(i) => self.switch_to(i, root_win),
            AddDecision::Add => {
                let i = self.ws.len();
                let ws = Workspace::new(path.to_path_buf(), i, sender.clone());
                // stack page name: unique — open paths are deduped
                self.stack
                    .add_named(&ws.hpane, Some(&ws.root.to_string_lossy()));
                self.ws.push(ws);
                // requirement 4: appended to the right of the previous one;
                // title_widget centers the strip as one entity (requirement 7).
                // the + rides LAST (strip order [pill … pill | +]): re-append
                // moves it back behind the fresh pill
                self.proj_box.remove(&self.add_btn);
                self.proj_box.append(&self.ws[i].btn);
                self.proj_box.append(&self.add_btn);
                // requirement 5: the cap kills "+" (menu paths still report it)
                self.add_btn.set_sensitive(self.ws.len() < MAX_WORKSPACES);
                let path_str = path.to_string_lossy();
                // persistence: the live strip IS the open set — rebuilt after
                // the push, it caps by construction (never exceeds it) and
                // prunes dead entries: a persisted ghost must never steal a
                // cap slot and evict a live workspace at boot (review finding,
                // S4's state-open-set-live-truth pins it). remember() carries
                // the recents/last_project side of the activation.
                let mut st = state::remember(state::load(), path_str.as_ref());
                st.open_projects = self
                    .ws
                    .iter()
                    .map(|w| w.root.to_string_lossy().into_owned())
                    .collect();
                state::save(&st);
                self.switch_to(i, root_win);
            }
        }
    }

    /// Header-pill click / an open's activation: flip the stack page — state
    /// carries over because NOTHING is re-rooted (the feature's core).
    fn switch_to(&mut self, i: usize, root: &gtk::ApplicationWindow) {
        if i >= self.ws.len() {
            return;
        }
        self.stack.set_visible_child(&self.ws[i].hpane);
        self.active = i;
        let path_str = self.ws[i].root.to_string_lossy().into_owned();
        // requirement 1: only the project name, in the WM title as well
        root.set_title(Some(&py_basename(&path_str)));
        state::save(&state::remember(state::load(), &path_str));
        for (j, w) in self.ws.iter().enumerate() {
            if j == i {
                w.btn.add_css_class("alpaca-on");
            } else {
                w.btn.remove_css_class("alpaca-on");
            }
        }
        // the shared header pills track the NEW workspace's run state
        self.sync_run_buttons();
        // a switch re-syncs git once (the set-root refresh's law, reused)
        self.ws[i].gitpanel.emit(GitPanelMsg::RefreshGit);
    }

    /// Run/Stop pills + tooltip follow the active workspace (its own run
    /// mirror + the per-root detect). Called on switch (and from RunStatus).
    fn sync_run_buttons(&mut self) {
        let Some(w) = self.ws.get(self.active) else {
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

    /// the running class rides the pill row — the ACTIVE workspace's mirror
    fn refresh_run_style(&self) {
        let running = self.ws.get(self.active).map(|w| w.running).unwrap_or(false);
        if running {
            self.run_row.set_css_classes(&["alpaca-runpill", "running"]);
        } else {
            self.run_row.set_css_classes(&["alpaca-runpill"]);
        }
    }
}

// basename(path.rstrip("/")) — empty result (root "/") → bare title
fn py_basename(s: &str) -> String {
    let t = s.trim_end_matches('/');
    t.rsplit('/').find(|c| !c.is_empty()).unwrap_or(t).to_owned()
}

/// 1586×992 clamped to the monitor (−40 top/side pad, −140
/// vertical incl. panel); a too-tall default opens half-offscreen. ponytail: monitor 0.
fn default_size() -> (i32, i32) {
    if let Some(disp) = gdk::Display::default() {
        let mons = disp.monitors();
        if mons.n_items() > 0
            && let Some(mon) = mons.item(0).and_downcast::<gdk::Monitor>() {
                let g = mon.geometry();
                return (1586.min(g.width() - 40), 992.min(g.height() - 140));
            }
    }
    (1586, 992)
}

/// The one header menu: workbench actions. Sections between groups are
/// what PopoverMenu renders as the separator lines — no separator rows.
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

fn open_project(root_win: &gtk::ApplicationWindow, sender: ComponentSender<App>) {
    let dialog = gtk::FileChooserNative::new(
        Some("Open Project"),
        Some(root_win),
        gtk::FileChooserAction::SelectFolder,
        Some("Open"),
        Some("Cancel"),
    );
    let sender = sender.clone();
    dialog.connect_response(move |d, resp| {
        let path = if resp == gtk::ResponseType::Accept {
            d.file().and_then(|f| f.path())
        } else {
            None
        };
        d.destroy();
        if let Some(p) = path
            && p.is_dir() {
                sender.input(AppMsg::OpenWorkspace(p));
            }
    });
    dialog.show();
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
        // OFF the response thread: create_project blocks up to 10s (git init
        // wall) — running it here would freeze the whole window. The Sender
        // is Send: the spawned thread posts straight back into the main
        // context.
        let snd = sender.clone();
        std::thread::spawn(move || {
            let res = runctl::create_project(&target);
            snd.input(AppMsg::ProjectCreated { target, res });
        });
    });
    d.present();
}

/// MessageDialog CLOSE destroy-on-response.
fn error_dialog(root: &gtk::ApplicationWindow, primary: &str, secondary: &str) {
    show_message(root, primary, Some(secondary), gtk::ButtonsType::Close);
}

/// MessageDialog is 4.10-deprecated upstream; the AlertDialog redesign is S3
/// polish. No message_type passed → INFO default; secondary set post-hoc
/// like the constructor kwarg.
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
        let app = app.clone();
        quit.connect_activate(move |_a, _p| app.quit());
    }
    app.add_action(&quit);

    // no-op stubs until workbenches land (they'll be the superproject layer
    // loading the up-to-3 projects): the rows must look enabled and do
    // nothing yet — these three become the wiring slots then.
    for name in ["new-workbench", "open-workbench", "save-workbench"] {
        let a = gio::SimpleAction::new(name, None);
        a.connect_activate(|_a, _p| {});
        app.add_action(&a);
    }

    // Surface-less for now (File/Edit/Window/Help rows removed 2026-10-14):
    // open-project/new-project/undo/redo/cut/copy/paste/select-all/
    // win-minimize/close/about/open-recent keep firing — the workbench
    // phase re-surfaces or deletes them when it defines its own menus.

    // undo/redo buffer-side, clipboard ops on the view's
    // own actions; routed to the ACTIVE workspace's editor
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
        let w = root_win.clone();
        close.connect_activate(move |_a, _p| w.destroy());
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
}

/// Launch entry: the component's Widgets struct is macro-private, so the
/// launch has to live beside the component (upstream's own examples do this).
pub fn run(root: Option<String>) {
    let root: Option<PathBuf> = root.filter(|p| Path::new(p).is_dir()).map(Into::into);
    // Empty argv: RelmApp's default (process argv) makes g_application_run
    // treat the project dir as a "file to open" → 'can not open files' critical.
    let relm = relm4::RelmApp::new("io.alpaca.rs").with_args(vec![]);
    crate::style::init();
    relm.run::<App>(root);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pb(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn add_decision_switch_add_full() {
        // requirement 5 + the dedupe rule: already open → switch (never add),
        // under cap → add, at cap → rejected
        let open = [pb("/a"), pb("/b")];
        assert_eq!(add_decision(&open, Path::new("/b")), AddDecision::Switch(1));
        assert_eq!(add_decision(&open, Path::new("/c")), AddDecision::Add);
        let three = [pb("/a"), pb("/b"), pb("/c")];
        assert_eq!(add_decision(&three, Path::new("/d")), AddDecision::Full);
        // empty boot: the first open always adds
        assert_eq!(add_decision(&[], Path::new("/a")), AddDecision::Add);
    }

    /// MenuModel item's attribute as a String (label / action).
    fn attr(model: &impl glib::object::IsA<gio::MenuModel>, name: &str, i: i32) -> Option<String> {
        use gio::prelude::MenuModelExt;
        model
            .item_attribute_value(i, name, None)
            .and_then(|v| v.get::<String>())
    }

    #[test]
    fn workbench_menu_sections_labels_actions() {
        use gio::prelude::MenuModelExt;
        // 3 sections = the two separator lines: [New, Open] | [Save] | [Exit]
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
