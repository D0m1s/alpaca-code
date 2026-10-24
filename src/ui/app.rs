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
    /// every open path (File▸Open Project…, recents, "+", New Project…,
    /// probe chains): switch if open / add under the cap / report when full
    OpenWorkspace(PathBuf),
    /// header-pill click → instant switch (requirement 8)
    SwitchWorkspace(usize),
    /// strip ✕ → close the tagged workspace (tombstone + swing)
    CloseWorkspace(usize),
    /// requirement 3: header "+" → the folder picker (shared open path)
    AddProjectDialog,
    /// launcher (2026-10-15): show the hub page (boot + menu Open Workbench)
    ShowLauncher,
    /// launcher card → adopt the file's uuid as this session's workbench,
    /// then route each surviving root through the shared open path
    LauncherOpen(String),
    /// launcher big + → the NAME popover commits first → folder picker; the
    /// accept opens the folder as a FRESH workbench (identity cleared) whose
    /// display name rides in wb_display until the FIRST save mints the file
    LauncherAdd(String),
    /// LauncherAdd's picker accept: clear the workbench identity, then open
    OpenNew(PathBuf, String),
    /// launcher pencil: display rewrite on the file (mtime preserved — the
    /// card order stays last-save-first); an adopted identity reloads its base
    LauncherRename { stem: String, display: String },
    /// launcher trash (confirmed in its popover): the file is deleted; an
    /// adopted identity is cleared so the next save mints FRESH (no resurrect)
    LauncherDelete(String),
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
    /// Ctrl+Shift+S / menu Save Workbench → the full explicit save
    SaveWorkbench,
    /// X-close: close-request already STOPPED the close; save, then destroy
    /// the window (`destroy()` does not re-emit close-request — no loop)
    SaveThenClose,
    /// Exit menu / Ctrl+Q → save, then quit the application
    SaveAndQuit,
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
    GitChanged(usize, crate::git::gitstatus::Rows),
    /// window notify::is-active → the ACTIVE workspace's git re-sync
    WindowActive,
    /// Edit menu → the active workspace's editor
    EditOp(EditKind),
    /// Ctrl+Shift+F → the active workspace's gitpanel search entry
    /// (tree mode: file search; changes mode: filter the changes rows)
    FocusSearch,
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
    /// PROBE-ONLY (ALPACA_PROBE_WB_SAVE) — workbench save-run chain: build a
    /// session, save, exercise the real close-request path
    ProbeWBSave(PathBuf),
    ProbeWBStep,
    /// PROBE-ONLY (ALPACA_PROBE_WB_RESTORE) — boot-restore asserts on the
    /// same rig config the SAVE run wrote
    ProbeWBRestore(PathBuf),
    ProbeWBRStep,
    /// PROBE-ONLY (ALPACA_PROBE_LN) — launcher boot + card-click chain
    ProbeLN(PathBuf),
    ProbeLNStep,
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

/// The slot the active lands on after closing `i`: the next live slot to the
/// right (browser convention), else the nearest live slot left, else None —
/// the zero-workspace state. Slots are append-only tombstones (vec never
/// compacts), so slot order == strip order and the captured `ws` tags on
/// every component output stay valid forever.
pub fn close_target(live: &[usize], i: usize) -> Option<usize> {
    live.iter()
        .find(|&&s| s > i)
        .copied()
        .or_else(|| live.iter().rev().find(|&&s| s < i).copied())
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
    /// editor | panes split — stored for the workbench save (the paned
    /// positions are the per-project layout lines)
    vpane: gtk::Paned,
    /// the header-strip pill (the close ✕ rides INSIDE its child box —
    /// one band covers label + ✕, user image #17)
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
    /// routes back into THIS workspace regardless of the active one. `sess`
    /// = the saved per-project session (workbench restore; None → defaults).
    fn new(path: PathBuf, ws: usize, app_out: ComponentSender<App>, sess: Option<state::ProjectSession>) -> Self {
        let path_str = path.to_string_lossy().into_owned();
        let sess = sess.unwrap_or_default();

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
        // workbench restore (S4 hidden-page law): a saved position is set at
        // CONSTRUCTION — the property survives the page's first allocation
        // (GTK clamps to min(saved, allocated−6) when undersized)
        if sess.vpane > 0 {
            vpane.set_position(sess.vpane);
        }

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
        if sess.hpane > 0 {
            hpane.set_position(sess.hpane);
        }

        // Requirement 2: the pill's idle look IS the old title look
        // (.alpaca-projbtn mirrors .alpaca-wintitle); hover = the branch pill's.
        let btn = gtk::Button::builder()
            .has_frame(false)
            .css_classes(["alpaca-projbtn"])
            .build();
        let lbl = gtk::Label::builder().label(py_basename(&path_str)).build();
        lbl.set_ellipsize(pango::EllipsizeMode::End);
        lbl.set_max_width_chars(24); // 3 long names must not blow the strip out
        // the strip ✕ (close-pill): INSIDE the pill, one band covering label
        // + ✕ (user image #17). NOT a nested Button — a Button-in-Button's
        // outer GestureClick claims the press on this build and the ✕ never
        // fires (measured: user click did nothing; the S4 probe's AppMsg
        // close worked, so only the click path was dead). A plain box with
        // its own GestureClick CLAIMED ON PRESS runs first (bubble: innermost
        // gesture sees the press first) and the pill click never races it.
        let close = gtk::Box::builder()
            .css_classes(["alpaca-projclose"])
            .tooltip_text("Close project")
            .build();
        // the patch is 18×18 (img 14 + 2×2 pad), a SQUARE hover band centered
        // in the pill (image #18 ruling; a patch that fills the 21px band is
        // a tall bar whose top inset ≠ its right inset)
        close.set_valign(gtk::Align::Center);
        if let Some(vec) = vector::icon("x-dim.svg") {
            let img = gtk::Image::from_paintable(Some(&vec));
            img.set_pixel_size(14);
            img.set_valign(gtk::Align::Center);
            close.append(&img);
        } else {
            // art loss (never on a shipped build): text stand-in
            close.append(&gtk::Label::builder().label("✕").build());
        }
        let click = gtk::GestureClick::new();
        let snd = app_out.clone();
        click.connect_pressed(move |g, _n, _x, _y| {
            // claim the sequence NOW: the pill's click gesture is denied for
            // this press (no switch), the ✕ closes its slot
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
        // requirement 6: every pill is clickable → instant switch
        let snd = app_out.clone();
        btn.connect_clicked(move |_b| snd.input(AppMsg::SwitchWorkspace(ws)));

        // tree first, then panes, editor Restore LAST (the old set_workspace
        // order — restore must ride after the re-roots it reads back)
        gitpanel.emit(GitPanelMsg::SetRoot(path.clone()));
        if !sess.expanded.is_empty() || !sess.changes_closed.is_empty() {
            // boot restore: gitpanel forwards the tree half DEFERRED (belt
            // for a future re-root with entry text — at boot the entry is
            // empty so set_text("") is a no-op; see gitpanel's SetTreeState)
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
    /// the workbench-menu chevron — chrome-hidden on the launcher page
    menu_btn: gtk::MenuButton,
    run_btn: gtk::Button,
    stop_btn: gtk::Button,
    run_row: gtk::Box,
    /// live by slot + tombstones: the close ✕ leaves `None` in its slot.
    /// Slots are NEVER shifted or reused — every component output carries the
    /// Add-time slot tag, and a shift would misroute them (the re-tag law).
    /// Slot order == strip order because slots only grow.
    ws: Vec<Option<Workspace>>,
    /// valid iff a live slot exists at active — never read when none live
    active: usize,
    /// the Workbench Launcher hub page (2026-10-15) — a stack page above the
    /// workspace pages; its outputs route through AppMsg's LauncherOpen/Add
    launcher: relm4::Controller<Launcher>,
    /// PROBE-ONLY (ALPACA_PROBE_S2) — step chain (workspace, step index);
    /// production never constructs it (env unset)
    probe_ws: Option<(PathBuf, usize)>,
    /// PROBE-ONLY (ALPACA_PROBE_S3) — step chain (workspace, step index);
    /// production never constructs it (env unset)
    probe_s3: Option<(PathBuf, usize)>,
    /// PROBE-ONLY (ALPACA_PROBE_S4) — step chain (workspace, step index);
    /// production never constructs it (env unset)
    probe_s4: Option<(PathBuf, usize)>,
    /// PROBE-ONLY (ALPACA_PROBE_WB_SAVE/_RESTORE) — the two workbench-run
    /// step chains; production never constructs them (env unset)
    probe_wb: Option<(PathBuf, usize)>,
    probe_wbr: Option<(PathBuf, usize)>,
    /// PROBE-ONLY (ALPACA_PROBE_LN) — the launcher chain
    probe_l: Option<(PathBuf, usize)>,
    /// the session's workbench (workbench step 2026-10-14): the LAST saved
    /// file, source of preserve-lists on the next save; None until first boot
    /// with a saved workbench
    wb: Option<state::WorkbenchFile>,
    /// the minted workbench name (uuid placeholder) — stable once saved
    wb_name: Option<String>,
    /// the display name awaiting the FIRST save of a fresh workbench (the
    /// `+` name prompt): save_all writes it into the minted file; files with
    /// a saved identity carry display INSIDE the file (wb.display) instead
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

        // clamped AFTER construction, same numbers; saved geometry wins
        // over the default (workbench step), clamped identically; maximized
        // is best-effort (Wayland honours are compositor-side)
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

        // The window's content = one stack page per open workspace
        // (requirement 8: transition None = instant, like browser tabs).
        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::None)
            .hexpand(true)
            .vexpand(true)
            .build();
        root.set_child(Some(&stack));

        // The launcher hub page (2026-10-15), named for the asserts and the
        // boot/menu-visible path. It's added FIRST — before any workspace
        // page — so a boot with nothing to open can simply make it visible.
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

        // Header chrome on the hub page (2026-10-15, user ruling): the
        // workbench-menu chevron and the project `+` are workspace furniture —
        // the launcher has its own big `+` card, so both vanish while the hub
        // is visible. One notify::visible-child hook covers every flip (boot /
        // open / switch / close-to-hub / ShowLauncher); the explicit set below
        // carries boot (the visible page is ALREADY the launcher — the queued
        // ShowLauncher is a no-op change and fires no notify). No component
        // sender captured → no relm4 shutdown-panic surface.
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

    // boot adopt (launcher 2026-10-15): wb_name requires the file to LOAD —
    // a stale last_workbench (manually deleted file) must not resurrect the
    // file at the next save (it would mint a fresh uuid instead)
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
            // boot adopt (launcher 2026-10-15): wb_name requires the file to
            // LOAD — a stale last_workbench (manually deleted file) must not
            // resurrect the file at the next save (it would mint a fresh
            // uuid instead). wb.name == stem for app-written files.
            wb: boot_wb,
            wb_name: boot_wb_name,
            wb_display: None,
        };

        register_actions(&relm4::main_application(), &root, sender.clone());

        // notify::is-active fires on EVERY notify (both False→True and the rare
        // True→False edge); each fire costs one sync refresh — keep exactly that.
        let in_active = sender.clone();
        root.connect_notify_local(Some("is-active"), move |_, _| {
            in_active.input(AppMsg::WindowActive)
        });

        // X-button save point (workbench step 2026-10-14). Close law: this can
        // fire at (or after) shutdown → guarded send. Stop ALWAYS — returning
        // Proceed lets GTK destroy the window before the queued AppMsg runs
        // (the relm4 input()-after-shutdown panic); SaveThenClose ends in
        // root.destroy(), which unlike close() does not re-emit close-request.
        let snd_close = sender.clone();
        root.connect_close_request(move |_| {
            let _ = snd_close.input_sender().send(AppMsg::SaveThenClose);
            glib::Propagation::Stop
        });

        // startup (2026-10-15): the Workbench Launcher AT EVERY BOOT (user
        // decision) — clicking a card adopts its saved session; the CLI arg
        // keeps the old direct-open path (no hub in between).
        if let Some(p) = arg {
            sender.input(AppMsg::OpenWorkspace(p));
        } else {
            sender.input(AppMsg::ShowLauncher);
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
        // PROBE-ONLY (ALPACA_PROBE_WB_SAVE/_RESTORE) — never in production
        if let Ok(dir) = std::env::var("ALPACA_PROBE_WB_SAVE") {
            sender.input(AppMsg::ProbeWBSave(PathBuf::from(dir)));
        }
        if let Ok(dir) = std::env::var("ALPACA_PROBE_WB_RESTORE") {
            sender.input(AppMsg::ProbeWBRestore(PathBuf::from(dir)));
        }
        // PROBE-ONLY (ALPACA_PROBE_LN) — never in production
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
                // the hub: rebuild its cards (a fresh disk scan every show),
                // flip the page, and hand the WM a bare title (no project)
                self.launcher.emit(LauncherMsg::Build);
                self.stack.set_visible_child_name("launcher");
                root.set_title(Some("alpaca-code"));
            }
            AppMsg::LauncherOpen(name) => self.open_workbench(&name, sender),
            AppMsg::LauncherAdd(name) => ask_folder(root, "New Workbench", sender, Some(name)),
            AppMsg::OpenNew(p, name) => {
                // the launcher's + path: a FRESH workbench identity — the
                // next Ctrl+Shift+S mints its uuid instead of riding the
                // previously-adopted workbench's file. The name prompt's
                // name rides in wb_display until that FIRST save writes it
                // into the minted file (empty → the folder basename).
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
                // the file's display rewrite (mtime preserved inside — the
                // card order stays last-save-first); the adopted base
                // reloads so the next save keeps the display
                state::set_workbench_display(&stem, &display);
                if self.wb_name.as_deref() == Some(stem.as_str()) {
                    self.wb = state::load_workbench(&stem);
                }
                self.launcher.emit(LauncherMsg::Build);
            }
            AppMsg::LauncherDelete(stem) => {
                state::delete_workbench(&stem);
                if self.wb_name.as_deref() == Some(stem.as_str()) {
                    // deleting the ADOPTED workbench: no identity — the next
                    // save mints FRESH instead of resurrecting the dead file
                    self.wb = None;
                    self.wb_name = None;
                    self.wb_display = None;
                }
                self.launcher.emit(LauncherMsg::Build);
            }
            AppMsg::AddProjectDialog => open_project(root, sender),
            AppMsg::OpenFile(ws, p) => {
                // filetree → editor routing within the tagged workspace
                if let Some(w) = self.ws.get_mut(ws).and_then(|o| o.as_mut()) {
                    w.editor.emit(EditorMsg::OpenFile(p));
                }
            }
            AppMsg::EditorState(ws, s) => {
                if let Some(w) = self.ws.get_mut(ws).and_then(|o| o.as_mut()) {
                    w.snapshot = s;
                }
                if self.live_len() > 0 {
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
            AppMsg::SaveWorkbench => self.save_all(root),
            AppMsg::SaveThenClose => {
                self.save_all(root);
                root.destroy(); // close-request already stopped; no re-emission
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
                // the run-status payload is not displayed; the
                // buttons + pill class follow the workspace's running mirror
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
                    self.run_btn.set_sensitive(false); // Stop-only until run ends
                    self.refresh_run_style();
                }
            }
            AppMsg::RunPressed => {
                // no workspace yet / live run → noop
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
                // run-off + style land with RunStarting (accepted) —
                // one mainloop turn, same frame
            }
            AppMsg::StopPressed => {
                // unconditional button sync, then the active workspace's panes
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
                // the diff open runs within the tagged workspace
                if let Some(w) = self.ws.get(ws).and_then(|o| o.as_ref()) {
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
                if let Some(w) = self.ws.get_mut(ws).and_then(|o| o.as_mut()) {
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
                if let Some(w) = self.ws.get_mut(ws).and_then(|o| o.as_mut())
                    && let Some((root, paths, msg)) = w.pending_commit.take() {
                        w.gitpanel.emit(GitPanelMsg::StartCommit { root, paths, msg });
                    }
            }
            AppMsg::GitChanged(ws, rows) => {
                // diff-tab renewal for the tagged workspace (a stale diff tab
                // across WORKSPACE switches does not exist anymore — each
                // workspace's editor carries its own diffs; restore rebuilds
                // ONLY path pages for the boot path)
                if let Some(w) = self.ws.get(ws).and_then(|o| o.as_ref()) {
                    let letters = rows.iter().map(|(rel, letter)| (rel.clone(), letter.clone())).collect();
                    w.editor.emit(EditorMsg::RefreshDiffs { root: w.root.clone(), letters });
                }
            }
            AppMsg::WindowActive => {
                // window focus re-syncs the ACTIVE workspace's git
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
                        say("two-open", self.ws.len() == 2 && self.live_len() == 2);
                        // THE feature asserts, live: state survives the flip.
                        // GObject PartialEq compares memory addresses
                        // (verified in glib-0.22 object.rs) — Widget vs Paned
                        // is the same instance's pointer, no upcast dance.
                        say(
                            "stack-visible-is-wsA",
                            self.stack
                                .visible_child()
                                .is_some_and(|v| v == self.ws[0].as_ref().unwrap().hpane),
                        );
                        say(
                            "editor-tabs-preserved",
                            // snapshot open_tabs are RELPATHS (editor emit_state)
                            self.ws[0].as_ref().unwrap().snapshot.open_tabs == vec!["a.txt".to_string()],
                        );
                        // the paned's first-allocation law: the position clamps
                        // to max = allocated_width − 6px separator — a narrow
                        // virtual display (broadway vfb) reads BELOW 1161 by
                        // exactly that clamp; assert the constructed position
                        // survived (no recompute/collapse), clamp included
                        let hw = self.ws[0].as_ref().unwrap().hpane.allocated_width();
                        let pos = self.ws[0].as_ref().unwrap().hpane.position();
                        println!("PROBE hpane-pos reads pos={pos} hpane-w={hw}");
                        // hw==0 → never allocated yet by assert time (racy on
                        // the frame clock): the property must read 1161.
                        // allocated (narrow vfb) → the clamp law applies.
                        let ok = if hw > 0 { pos == 1161.min(hw - 6) } else { pos == 1161 };
                        say("hpane-pos-1161", ok);
                        say(
                            "on-class-wsA",
                            self.ws[0].as_ref().unwrap().btn.has_css_class("alpaca-on")
                                && !self.ws[1].as_ref().unwrap().btn.has_css_class("alpaca-on"),
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
                            self.ws.iter().flatten().map(|w| w.btn.allocated_height()).collect();
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
                                == Some(self.ws[0].as_ref().unwrap().root.file_name().unwrap().to_str().unwrap()),
                        );
                    }
                    3 => {
                        let ws_c = ws_dir.to_string_lossy().replace("wsA", "wsC");
                        self.open_workspace(std::path::Path::new(&ws_c), root, sender.clone());
                        say("three-open", self.ws.len() == 3 && self.live_len() == 3);
                        say("add-btn-disabled", !self.add_btn.is_sensitive());
                        // state.json's open set must mirror the LIVE strip:
                        // the rig seeds a ghost dir (GONE, deleted) — boot
                        // filters it from the strip, and this ADD must prune
                        // it from the open set (a persisted ghost steals a
                        // cap slot and evicts live workspaces at next boot)
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
                        // active is wsC now; swing back to wsA via the real
                        // switch path
                        self.switch_to(0, root);
                    }
                    // requirement: an already-open path must SWITCH (the
                    // add_decision dedupe), never duplicate the strip pill
                    4 => {
                        self.open_workspace(ws_dir.as_path(), root, sender.clone());
                        say("re-open-wsA-switches", self.live_len() == 3 && self.active == 0);
                    }
                    // CLOSE-pill step: wsA (slot 0) dies under the real ✕
                    // path — tombstone + active swing right (slot 0 of
                    // [0,1,2] → 1 = wsB), strip unit gone, "+" re-armed
                    5 => {
                        self.close_workspace(0, root, sender.clone());
                        // wsB (slot 1) + wsC (slot 2) stay live; slots never
                        // compact: 3,0-tombstone,1,2
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
                        // wsB's pill carries the active class; wsA's unit LEFT
                        say(
                            "close-pill-class",
                            self.ws[1].as_ref().unwrap().btn.has_css_class("alpaca-on"),
                        );
                        // strip = [wsB unit | wsC unit | +]
                        say("close-pill-rm", self.proj_box.observe_children().n_items() == 3);
                        say("close-reopen-plus", self.add_btn.is_sensitive());
                        // state: wsA pruned from the open set (live-truth law)
                        let wsb = ws_dir.to_string_lossy().replace("wsA", "wsB");
                        let wsb_state = wsb.clone(); // closure gets its own copy
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
                        // THE re-tag proof — a component output must still
                        // route into slot 1's workspace after slot 0 died
                        // (a shifted vec would miss it). b2.txt is rig-only.
                        sender.input(AppMsg::OpenFile(
                            1,
                            PathBuf::from(&wsb).join("b2.txt"),
                        ));
                        // tombstone-safe routing: a message aimed at slot 0
                        // must no-op (no panic — the guard IS the assert)
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
                // PROBE-ONLY (ALPACA_PROBE_WB_SAVE) — never runs in production
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
                        // a REAL layout for the vfb before the paned steps (the
                        // S3 floor-then-set law: set_position garbles against a
                        // stale allocation)
                        root.set_size_request(1586, 992);
                    }
                    1 => {
                        let Some(w) = self.ws.get(self.active).and_then(|o| o.as_ref()) else { return };
                        w.vpane.set_position(650);
                        w.hpane.set_position(900);
                        // expand rig row 1 = "sub" (scan_dir sorts dirs first:
                        // [other, sub, a.txt]) — row_expanded inserts the row's
                        // abs PATH into `open`, the save's truth source
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
                    3 => sender.input(AppMsg::SaveWorkbench), // the real funnel
                    4 => {
                        // disk truth: the workbench file + state.json extras.
                        // The uuid name is unknowable here — read the dir's one
                        // workbenches/<uuid>.json (S4's grep-on-disk style)
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
                        // in-process readback of the live sources behind it
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
                        // the REAL X path: close-request → guarded send →
                        // SaveThenClose (also proves Stop-first ordering)
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
                // PROBE-ONLY (ALPACA_PROBE_WB_RESTORE) — same XDG rig config
                // as the SAVE run (its state.json + workbenches/ feed boot)
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
                        // 2026-10-15: boot no longer auto-opens — the hub
                        // shows. Click its single card through the REAL route
                        // (ProbeClick → the Button's activation → output →
                        // LauncherOpen → shared open path + session restore)
                        self.launcher.model().probe_click(0);
                    }
                    1 => {
                        // boot restored the strip: 1 live ws + active swing
                        say("r-strip", self.live_len() == 1 && self.active == 0);
                        say("r-page-flipped", self.stack.visible_child_name().as_deref() != Some("launcher"));
                        say(
                            "r-adopted",
                            self.wb_name.as_deref()
                                == self.launcher.model().entries.first().map(|e| e.name.as_str()),
                        );
                        if let Some(w) = self.ws.get(0).and_then(|o| o.as_ref()) {
                            say("r-tabs", w.snapshot.open_tabs == vec!["a.txt".to_string()]);
                            // paned restore, S4's pinned law: an allocated page
                            // clamps at save vs alloc−6; unallocated holds the
                            // property. ws_dir kept for the compiler only.
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
                            // editor scroll: saved (rel, frac); restore_frac
                            // re-applied on the adj bounds change — frac is
                            // scale-free, >0.4 tolerates layout drift
                            let sc = w.editor.model().scroll_state();
                            say(
                                "r-scroll",
                                sc.as_ref().map(|(r, f)| r == "a.txt" && *f > 0.4)
                                    .unwrap_or(false),
                            );
                            println!("PROBE restore hpane {hp}/{ha} vpane {vp}/{vh} scroll {sc:?}");
                            // the deferred RestoreTree replayed the open set
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
                // PROBE-ONLY (ALPACA_PROBE_LN) — never runs in production
                self.probe_l = Some((dir, 0));
                root.present();
                let snd = sender.clone();
                glib::timeout_add(std::time::Duration::from_millis(PROBE_LN_MS[0]), move || {
                    snd.input(AppMsg::ProbeLNStep);
                    glib::ControlFlow::Break
                });
            }
            AppMsg::ProbeLNStep => {
                // PROBE-ONLY (ALPACA_PROBE_LN) — launcher boot + card clicks;
                // emissions target the launcher's real widgets
                let Some((_rig, step)) = self.probe_l.as_ref().map(|(w, s)| (w.clone(), *s)) else {
                    return;
                };
                let say = |name: &str, ok: bool| {
                    println!("PROBE assert: {name}: {}", if ok { "PASS" } else { "FAIL" })
                };
                // rig (wrapper-built): projA/.git = main, projB/.git =
                // feature-ln; aaaabbbb-0001.json = projA(c-sharp) + projB
                // (typescript) written LAST (newer mtime = FIRST card),
                // aaaabbbb-0002.json = projB(rust) alone
                match step {
                    0 => {
                        // boot truth: hub page up, 2 cards, the tiles' facts
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
                        // hub chrome law: workspace furniture hidden AT BOOT
                        say(
                            "ln-chrome-hidden",
                            !self.menu_btn.is_visible() && !self.add_btn.is_visible(),
                        );
                        // boot adopt filter (launcher 2026-10-15): this rig's
                        // state.json points last_workbench at a MISSING file —
                        // wb_name must stay None so the next save mints FRESH
                        // (no resurrection identity for the dead file)
                        say("ln-boot-stale-no-adopt", self.wb_name.is_none());
                    }
                    1 => {
                        // the REAL click: card 0's activation output →
                        // LauncherOpen → adopt + open both roots
                        self.launcher.model().probe_click(0);
                    }
                    2 => {
                        println!("PROBE LN: open asserts");
                        // the card's 2 roots via the shared Add path; the
                        // LAST processed root (projB, alphabetical) is active
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
                        // and the chrome is back on any project page
                        say(
                            "ln-chrome-shown",
                            self.menu_btn.is_visible() && self.add_btn.is_visible(),
                        );
                    }
                    3 => {
                        // card 1 (projB only) while 2 live → blend: switch
                        self.launcher.model().probe_click(1);
                    }
                    4 => {
                        // blending law: an already-open root SWITCHES (no
                        // strip growth); the identity swaps to the clicked file
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
                        // hub again + the rename drive part 1: open the pencil
                        // card's entry popover (the REAL builder, icons
                        // revealed via the hover law the probe replicates) and
                        // set the text — the commit rides in step 7 so
                        // ALPACA_PROBE_LN_HOLD=7 captures the OPEN popover
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
                        // rename drive part 2: the REAL commit on step 6's
                        // popover — the SAME entry, no rebuild (reopening
                        // would lose the typed text)
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
                        // the ADOPTED card's rename rides the same chain —
                        // the LauncherRename arm must RELOAD wb so the next
                        // save keeps the display (the Ctrl+S path)
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
                        // card relabeled from the REBUILT strip + the file's
                        // display field; adopted identity (0002) untouched
                        let texts = self.launcher.model().probe_texts();
                        say("ln-rename-label", texts.first().is_some_and(|(n, _)| n == "Nice wb"));
                        say(
                            "ln-rename-file",
                            state::load_workbench("aaaabbbb-0001")
                                .is_some_and(|w| w.display.as_deref() == Some("Nice wb")),
                        );
                        // the ADOPTED file's display: on disk AND in the
                        // reloaded base (the next save preserves it)
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
                        // the hub page never left during the edit
                        say(
                            "ln-rename-page",
                            self.stack.visible_child_name().as_deref() == Some("launcher"),
                        );
                    }
                    9 => {
                        // delete drive on the ADOPTED card (wb_name =
                        // aaaabbbb-0002 = strip index 1): open THE confirm
                        // popover, then click its real Delete button
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
                        // the file is gone (only the renamed 0001 card left)
                        // and the adopted identity was CLEARED: the next save
                        // mints fresh — no resurrection of the dead file
                        say("ln-del-card-gone", texts.len() == 1 && texts[0].0 == "Nice wb");
                        say(
                            "ln-del-file-gone",
                            state::load_workbench("aaaabbbb-0002").is_none(),
                        );
                        say("ln-del-adopt-cleared", self.wb_name.is_none() && self.wb.is_none());
                        // no identity → a save here would mint FRESH
                        say("ln-del-fresh-rearm", self.wb_display.is_none());
                    }
                    11 => {
                        // + flow: the REAL PlusClick routes through update()
                        // (the previous prompt's teardown law lives there);
                        // its COMMIT opens the native picker — never driven
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
                        // then the identity path via OpenNew with a name
                        let rig = self
                            .probe_l
                            .as_ref()
                            .map(|(d, _)| d.clone())
                            .unwrap_or_default();
                        sender.input(AppMsg::OpenNew(rig.join("projA"), "My New Wb".into()));
                    }
                    13 => {
                        println!("PROBE LN: create asserts");
                        // projA already live → the shared open path SWITCHES
                        // (no strip growth); fresh identity + the name rides
                        // in wb_display until the FIRST save
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
                // PROBE-ONLY capture gate (ALPACA_PROBE_LN_HOLD=N): run steps
                // 0..=N-1, then park the app — the S2_HOLD precedent (staged
                // pixel capture: icons revealed + popover open). The probe
                // fns reveal the hover icons themselves. Production never
                // reads this (env unset).
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
                        // XWayland path: KWin need not honor default_size and
                        // 900/850 must straddle the legal range (hpane 677 at
                        // the wide-trace = both rounds clamp identically) —
                        // floor the window BEFORE the wide split (a same-step
                        // floor+set_position garbles the paned position:
                        // readback came back 1568, the split never applied)
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
const PROBE_S4_STEP_MS: [u64; 8] = [500, 700, 700, 700, 700, 500, 700, 700];

/// PROBE-ONLY (ALPACA_PROBE_WB_SAVE) — gap[i] = ms delay before step i
const PROBE_WB_SAVE_MS: [u64; 6] = [500, 700, 700, 700, 700, 700];
/// PROBE-ONLY (ALPACA_PROBE_LN) — launcher chain: 0 boot asserts, 1 click,
/// 2 open asserts, 3 blend-click, 4 blend asserts, 5 DONE
const PROBE_LN_MS: [u64; 15] = [
    1300, 800, 800, 800, 800, 700, 900, 900, 900, 900, 900, 900, 900, 900, 700,
];
/// PROBE-ONLY (ALPACA_PROBE_WB_RESTORE) — 0 = click the launcher card,
/// 1 = restore asserts, 2 = DONE
const PROBE_WB_RESTORE_MS: [u64; 3] = [500, 1400, 700];

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
                if let Some(w) = self.ws.get(self.active).and_then(|o| o.as_ref()) {
                    w.gitpanel.emit(GitPanelMsg::ModeChanges);
                }
            }
            // commit_clicked rejects an empty entry — fill the real entry
            2 => {
                if let Some(w) = self.ws.get(self.active).and_then(|o| o.as_ref()) {
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
                if let Some(w) = self.ws.get(self.active).and_then(|o| o.as_ref()) {
                    w.editor.emit(EditorMsg::ProbeClip);
                }
            }
            // typing steps — fill the real search entry; GtkSearchEntry's
            // delayed "search-changed" fires the REAL EntryChanged route.
            // Pre-fix the changes arm re-fetches (SYNC git per keystroke);
            // its stall straddles the next typing step's timer — deltas ≈
            // 150ms delay + shim sleep (green ≈ the 400ms gap).
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
            // clear the needle so the masthead sees the full set again;
            // ProbeDiffScroll rides the same PRE-SWITCH step (the diff page is
            // ~2s old here and settled — step 4 timing is racy, measured;
            // step 12's open closes nothing — it ADDS wsB, so the pre-switch
            // window is the settled slot)
            9 => {
                if let Some(w) = self.ws.get(self.active).and_then(|o| o.as_ref()) {
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
                if let Some(w) = self.ws.get(self.active).and_then(|o| o.as_ref()) {
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
                if let Some(w) = self.ws.get(self.active).and_then(|o| o.as_ref()) {
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
        // canonical spelling (fix): a trailing-/ or `..` variant of an open
        // project must SWITCH, not open a duplicate strip page — add_decision
        // and BootActivate compare spellings. Missing dir → raw fallback.
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        // live slots in strip order parallel the decision's indices (tombstones
        // skip — Switch(i) maps through open_slots, not the vec position)
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
                let i = self.ws.len(); // next slot (slots grow, never reuse)
                let sess = self
                    .wb
                    .as_ref()
                    .and_then(|w| w.projects.get(path.to_string_lossy().as_ref()))
                    .cloned();
                let ws = Workspace::new(path.to_path_buf(), i, sender.clone(), sess);
                // stack page name: unique — open paths are deduped
                self.stack
                    .add_named(&ws.hpane, Some(&ws.root.to_string_lossy()));
                self.ws.push(Some(ws));
                // requirement 4: appended to the right of the previous one;
                // title_widget centers the strip as one entity (requirement 7).
                // the + rides LAST (strip order [pill … pill | +]): re-append
                // moves it back behind the fresh pill
                self.proj_box.remove(&self.add_btn);
                self.proj_box.append(&self.ws[i].as_ref().unwrap().btn);
                self.proj_box.append(&self.add_btn);
                // requirement 5: the cap kills "+" (menu paths still report it)
                self.add_btn.set_sensitive(self.live_len() < MAX_WORKSPACES);
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
                    .flatten()
                    .map(|w| w.root.to_string_lossy().into_owned())
                    .collect();
                state::save(&st);
                self.switch_to(i, root_win);
            }
        }
    }

    /// The launcher's card click (2026-10-15): ADOPT the saved file as this
    /// session's workbench (self.wb/wb_name — the next Ctrl+Shift+S saves
    /// into THAT uuid's file), then route each surviving root through the
    /// shared OpenWorkspace (already-open → switch; under-cap → add w/ its
    /// ProjectSession restore; cap-full reports). root order = open_roots
    /// (alphabetical, state's last_project last = it lands active).
    fn open_workbench(&mut self, name: &str, sender: ComponentSender<Self>) {
        let Some(wb) = state::load_workbench(name) else {
            return;
        };
        let roots = crate::ui::launcher::open_roots(
            &wb,
            state::load().last_project.as_deref().map(Path::new),
        );
        if roots.is_empty() {
            return; // every project vanished — stay on the launcher
        }
        self.wb = Some(wb);
        self.wb_name = Some(name.to_owned());
        for p in roots {
            sender.input(AppMsg::OpenWorkspace(p));
        }
    }

    /// Header-pill click / an open's activation / a close's active-swing:
    /// flip the stack page — state carries over because NOTHING is re-rooted
    /// (the feature's core). Tombstone slots guard-return.
    fn switch_to(&mut self, i: usize, root: &gtk::ApplicationWindow) {
        let Some(w) = self.ws.get(i).and_then(|o| o.as_ref()) else {
            return;
        };
        let path_str = w.root.to_string_lossy().into_owned();
        let hpane = w.hpane.clone(); // owned ref — no borrow into the mutations
        self.stack.set_visible_child(&hpane);
        self.active = i;
        // requirement 1: only the project name, in the WM title as well
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
        // the shared header pills track the NEW workspace's run state
        self.sync_run_buttons();
        // a switch re-syncs git once (the set-root refresh's law, reused)
        if let Some(w) = self.ws.get(i).and_then(|o| o.as_ref()) {
            w.gitpanel.emit(GitPanelMsg::RefreshGit);
        }
    }

    /// live workspaces (tombstones skipped) — cap checks, FlushTabs, "+" gate
    fn live_len(&self) -> usize {
        self.ws.iter().flatten().count()
    }

    /// The ONE workbench save (2026-10-14): tabs flush (defeats the 1s
    /// debounce — its last event before quit is lost), window geometry, the
    /// workbenches/<uuid>.json session file, then state.json extras.
    /// Best-effort throughout — the state contract means it never raises.
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
        // display: a fresh-first save consumes wb_display (the `+` name
        // prompt); every later save preserves the FILE's display (launcher
        // renames rewrite the file + reload wb — self.wb carries the latest)
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
        // sessions survive a project CLOSE this session: previous entries are
        // the base, live ones insert-overwrite (stale entries on never-reopened
        // projects harm nothing — boot filters by p.is_dir())
        if let Some(old) = self.wb.take() {
            wb.projects = old.projects;
            if !minted {
                wb.display = old.display; // the adopted file's display survives
            }
        }
        for w in self.ws.iter().flatten() {
            // live read, no mirror: gitpanel forwards both trees, the editor
            // model carries the active file's scroll (panes.model() precedent)
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
        self.wb = Some(wb); // next save's preservation base
    }

    /// live slot numbers in strip order — slots are append-only, so slot
    /// order == strip order; parallel to add_decision's open indices
    fn live_slots(&self) -> Vec<usize> {
        self.ws
            .iter()
            .enumerate()
            .filter_map(|(s, w)| w.is_some().then_some(s))
            .collect()
    }

    /// The strip ✕: tombstone the slot (indices are Add-time and never shift
    /// — the tags every component output carries stay valid), kill this
    /// workspace's pane children via the real Stop/kill paths, drop the
    /// assembly, remove the stack page + strip unit, swing the active (next
    /// live slot right, else nearest left, else zero-workspace), then
    /// rebuild the persisted open set from the LIVE strip (same live-truth
    /// law as Add — and the closed project's per-project tabs entry STAYS so
    /// a reopen restores its pages).
    fn close_workspace(&mut self, slot: usize, root: &gtk::ApplicationWindow, sender: ComponentSender<Self>) {
        let Some(w) = self.ws.get_mut(slot).and_then(|o| o.take()) else {
            return;
        };
        // kill every pane child SYNCHRONOUSLY before the drop (agent/term are
        // session leaders — they outlive the component drop otherwise). The
        // kill rides the model's own pids: relm4's shutdown branch races
        // queued messages in futures::select! and can drop them, so no
        // PanesMsg round-trip here.
        for pid in w.panes.model().live_pids() {
            kill_tree(pid);
        }
        self.stack.remove(&w.hpane);
        self.proj_box.remove(&w.btn);
        drop(w); // controllers drop → component runtimes shut down
        // active swing: the tombstone is already in place, so live_slots()
        // sees the post-close strip
        if slot == self.active {
            match close_target(&self.live_slots(), slot) {
                Some(next) => self.switch_to(next, root),
                None => {
                    // zero workspaces: the fresh-install state — no title, no
                    // run pills, and boot's open_projects fallback must NOT
                    // resurrect the closed project (open_projects = [] below).
                    // 2026-10-15: that state IS the launcher now
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
        // live remain → the persisted open set is the live strip (the
        // zero-live case wrote its [] in the swing arm above)
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

    /// Run/Stop pills + tooltip follow the active workspace (its own run
    /// mirror + the per-root detect). Called on switch (and from RunStatus).
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

    /// the running class rides the pill row — the ACTIVE workspace's mirror
    fn refresh_run_style(&self) {
        let running = self.ws.get(self.active).and_then(|o| o.as_ref()).map(|w| w.running).unwrap_or(false);
        if running {
            self.run_row.set_css_classes(&["alpaca-runpill", "running"]);
        } else {
            self.run_row.set_css_classes(&["alpaca-runpill"]);
        }
    }
}

// basename(path.rstrip("/")) — empty result (root "/") → bare title.
// pub(crate): the launcher's project tiles read the same spelling.
pub(crate) fn py_basename(s: &str) -> String {
    let t = s.trim_end_matches('/');
    t.rsplit('/').find(|c| !c.is_empty()).unwrap_or(t).to_owned()
}

/// (w,h) clamped to the monitor (−40 top/side pad, −140 vertical incl.
/// panel); a too-tall default opens half-offscreen. ponytail: monitor 0.
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

/// The shared folder picker. `fresh` = the launcher's New-Workbench path —
/// the accept clears the workbench identity BEFORE the open (a fresh uuid
/// on the next save) instead of adding to the adopted one.
fn ask_folder(
    root_win: &gtk::ApplicationWindow,
    title: &str,
    sender: ComponentSender<App>,
    // Some(display name) = the fresh path (the launcher + name prompt rides
    // in) → OpenNew; None = the plain adopt-open path → OpenWorkspace
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
        // workbench step: quit saves the session first (Ctrl+Q + the menu's
        // Exit row both target app.quit)
        let sender = sender.clone();
        quit.connect_activate(move |_a, _p| sender.input(AppMsg::SaveAndQuit));
    }
    app.add_action(&quit);

    // the workbench phase's stubs got REAL targets (launcher, 2026-10-15):
    // New Workbench = the hub's + (its name prompt → the picker → a fresh
    // identity); Open Workbench = the hub. One naming path, no menu shortcut.
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
        // Save Workbench (menu row + Ctrl+Shift+S) → the full explicit save
        let sender = sender.clone();
        let a = gio::SimpleAction::new("save-workbench", None);
        a.connect_activate(move |_a, _p| sender.input(AppMsg::SaveWorkbench));
        app.add_action(&a);
    }
    {
        // Ctrl+Shift+F → the active workspace's search entry (no menu
        // surface; EditOp-style active-slot routing)
        let sender = sender.clone();
        let a = gio::SimpleAction::new("focus-search", None);
        a.connect_activate(move |_a, _p| sender.input(AppMsg::FocusSearch));
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
        // the X button's exact path (close-request → guarded send → stop) —
        // one exit behavior for accelerator and programmatic callers
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

/// Launch entry: the component's Widgets struct is macro-private, so the
/// launch has to live beside the component (upstream's own examples do this).
pub fn run(root: Option<String>) {
    let root: Option<PathBuf> = root.filter(|p| Path::new(p).is_dir()).map(Into::into);
    // Empty argv: RelmApp's default (process argv) makes g_application_run
    // treat the project dir as a "file to open" → 'can not open files' critical.
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
        // next live slot right (browser convention), else leftmost live, else
        // None = zero-workspace state. Slots are append-only tombstones, so
        // the live list is in strip order and never reused.
        assert_eq!(close_target(&[0, 1], 0), Some(1));
        assert_eq!(close_target(&[0, 1], 1), Some(0));
        assert_eq!(close_target(&[0, 1, 2], 1), Some(2));
        assert_eq!(close_target(&[0, 1, 2], 2), Some(1));
        // tombstoned slots never reuse: live list only
        assert_eq!(close_target(&[1, 3], 1), Some(3));
        assert_eq!(close_target(&[1, 3], 3), Some(1));
        // the last one standing → close-to-empty
        assert_eq!(close_target(&[2], 2), None);
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
