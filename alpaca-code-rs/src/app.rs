//! App component — the window shell. Port of alpaca_code/window.py: headerbar
//! (File/Edit/Window/Help label buttons + run/stop cells), hpaned → vpaned
//! (editor / panes placeholder cards) + file-browser card, workspace lifecycle
//! (set_workspace / discard flow), menus/actions, and the persist-tabs
//! debounce. Slot cards stay in the model; Tasks 6–8 pack real components.

// 4.10-deprecated Dialog/FileChooser/MessageDialog API — the new-project and
// open-file dialogs mirror window.py's Gtk flow; S3 polish modernizes.
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
    SetWorkspace(PathBuf),
    /// tree → editor routing; wired when editor lands (t6)
    OpenFile(PathBuf),
    /// editor state flip → (a) dirty gate for set_workspace, (b) arms the
    /// persist-tabs debounce
    EditorState(EditorSnapshot),
    /// panes → header Run/Stop sync (t8: launch accepted — the starting window)
    RunStarting,
    RunStatus(bool),
    /// the run/stop pills (window.py:_on_run/_on_stop)
    RunPressed,
    StopPressed,
    FlushTabs,
    /// open-recent target no longer exists → info dialog
    RecentMissing(String),
    /// discard-dialog accepted → switch without the dirty gate
    DiscardWorkspace(PathBuf),
    /// CHANGES row → editor diff page (window.py:_open_changes_diff)
    OpenDiff {
        rel: String,
        letter: String,
    },
    /// gitview commit button → the flush chain (Commit → SaveOpen → Flushed →
    /// StartCommit)
    Commit {
        root: PathBuf,
        paths: Vec<String>,
        msg: String,
    },
    /// editor SaveOpen's ack — the ONLY thing that releases pending_commit
    Flushed,
    /// git rows → editor diff-tab renewal (window.py:_on_git_changed)
    GitChanged(crate::gitstatus::Rows),
    /// window notify::is-active → git re-sync (filetree.py:refresh_git)
    WindowActive,
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
}

thread_local! {
    /// PROBE-ONLY (ALPACA_PROBE_S3) keeper tick counter for the width traces
    static PROBE_S3_TICKS: Cell<u32> = const { Cell::new(0) };
}

#[derive(Clone)]
struct HeaderButtons {
    file: gtk::MenuButton,
    edit: gtk::MenuButton,
    window: gtk::MenuButton,
    help: gtk::MenuButton,
}

struct App {
    root: Option<String>,
    title: gtk::Label,
    run_btn: gtk::Button,
    stop_btn: gtk::Button,
    run_row: gtk::Box,
    menu_btns: HeaderButtons,
    /// Task 8: the panes component (Agent/Terminal/Output + run lifecycle)
    panes: relm4::Controller<Panes>,
    /// Task 6: the git panel card (filetree + changes + branch pill behind one
    /// chrome) — kept for its input sender + widget
    gitpanel: relm4::Controller<GitPanel>,
    /// T7 — the flush chain: Commit → SaveOpen → Flushed → StartCommit
    pending_commit: Option<(PathBuf, Vec<String>, String)>,
    /// PROBE-ONLY (ALPACA_PROBE_S2) — step chain (workspace, step index);
    /// production never constructs it (env unset)
    probe_ws: Option<(PathBuf, usize)>,
    /// PROBE-ONLY (ALPACA_PROBE_S3) — step chain (workspace, step index);
    /// production never constructs it (env unset)
    probe_s3: Option<(PathBuf, usize)>,
    dirty: bool,
    /// latest editor snapshot, read by the debounce flush (window.py reads the
    /// editor live; a snapshot at every flip is equivalent)
    editor_state: EditorSnapshot,
    /// panes' has_running_run — Task 8 keeps it honest
    running: bool,
    /// Task 6: the editor component (kept for its input sender + widget)
    editor: relm4::Controller<Editor>,
}

#[relm4::component]
impl Component for App {
    type CommandOutput = ();
    type Input = AppMsg;
    type Output = ();
    type Init = Option<PathBuf>;

    view! {
        gtk::ApplicationWindow {
            set_title: Some("alpaca_code"),
            set_icon_name: Some("io.alpaca.rs"),
        }
    }

    fn init(model: Self::Init, root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        let title = gtk::Label::new(Some("alpaca-code"));
        title.set_css_classes(&["alpaca-wintitle"]);
        // a long project name must not widen the window's min
        title.set_ellipsize(pango::EllipsizeMode::End);

        let menu_btns = HeaderButtons {
            file: gtk::MenuButton::new(),
            edit: gtk::MenuButton::new(),
            window: gtk::MenuButton::new(),
            help: gtk::MenuButton::new(),
        };
        menu_btns.file.set_label("File");
        menu_btns.edit.set_label("Edit");
        menu_btns.window.set_label("Window");
        menu_btns.help.set_label("Help");
        no_arrow(&menu_btns.file);
        no_arrow(&menu_btns.edit);
        no_arrow(&menu_btns.window);
        no_arrow(&menu_btns.help);

        // Mockup's pill buttons: green play / red stop — design SVG (theme
        // symbols grey both out); t4's symbolic stand-in paid off — badges (t5)
        // carries the art. valign CENTER: default FILL lands ink top-left
        // (window.py:_img, measured 7px high).
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
        header.set_title_widget(Some(&title));
        // design's menu row: File Edit Window Help
        header.pack_start(&menu_btns.file);
        header.pack_start(&menu_btns.edit);
        header.pack_start(&menu_btns.window);
        header.pack_start(&menu_btns.help);

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
        // band = 28 exactly BECAUSE the row stands 28 tall (CENTER in the 32px
        // bar = 2px air each side); header interior would FILL it to 32.
        run_row.set_valign(gtk::Align::Center);
        header.pack_end(&run_row);
        root.set_titlebar(Some(&header));

        // run/stop pills (window.py:_on_run/_on_stop route through the model)
        let send = sender.clone();
        run_btn.connect_clicked(move |_b| send.input(AppMsg::RunPressed));
        let send = sender.clone();
        stop_btn.connect_clicked(move |_b| send.input(AppMsg::StopPressed));

        // Task 6: editor card — the component carries its own alpaca-card
        let editor_slot = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let app_in = sender.clone();
        let editor = Editor::builder()
            .launch(())
            .connect_receiver(move |_, out| match out {
                EditorOutput::StateChanged(s) => {
                    app_in.input(AppMsg::EditorState(s));
                }
                EditorOutput::Flushed => app_in.input(AppMsg::Flushed), // the flush chain's ack
            });
        let ed_in = editor.sender().clone();
        editor_slot.append(editor.widget());
        // Task 8: the real panes card (the placeholder is gone)
        let app_in = sender.clone();
        let panes = Panes::builder()
            .launch(())
            .connect_receiver(move |_, out| match out {
                PanesOutput::RunStarting => app_in.input(AppMsg::RunStarting),
                PanesOutput::RunStatus { running } => app_in.input(AppMsg::RunStatus(running)),
            });
        let panes_slot = gtk::Box::new(gtk::Orientation::Vertical, 0);
        panes_slot.append(panes.widget());
        let vpane = gtk::Paned::new(gtk::Orientation::Vertical);
        vpane.set_wide_handle(true);
        vpane.set_start_child(Some(&editor_slot));
        vpane.set_end_child(Some(&panes_slot));
        // design: editor card 56..649 (593) − pane handle overlap
        vpane.set_position(592);

        // git panel card (window.py:165 — FileBrowser in the hpane end child)
        let app_in = sender.clone();
        let gitpanel = GitPanel::builder()
            .launch(())
            .connect_receiver(move |_, out| match out {
                GitPanelOutput::OpenFile(p) => {
                    app_in.input(AppMsg::OpenFile(p));
                }
                GitPanelOutput::OpenDiff { rel, letter } => {
                    let _ = app_in.input(AppMsg::OpenDiff { rel, letter });
                }
                GitPanelOutput::Commit { root, paths, msg } => {
                    let _ = app_in.input(AppMsg::Commit { root, paths, msg });
                }
                GitPanelOutput::GitChanged(rows) => {
                    let _ = app_in.input(AppMsg::GitChanged(rows));
                }
            });

        // hpane DIRECTLY as the window child — a wrapping Box under-allocates
        // (window.py:148, measured 43px tall)
        let hpane = gtk::Paned::new(gtk::Orientation::Horizontal);
        hpane.set_wide_handle(true);
        hpane.set_start_child(Some(&vpane));
        hpane.set_end_child(Some(gitpanel.widget()));
        // design: cards gap columns 1172..1180 → pane pos = card end − 10
        hpane.set_position(1161);
        root.set_child(Some(&hpane));

        // window.py:37-46 — clamped AFTER construction, same numbers
        let widgets = view_output!();
        let (w, h) = default_size();
        root.set_default_size(w, h);

        let model = App {
            root: model.as_ref().map(|p| p.to_string_lossy().into_owned()),
            title,
            run_btn,
            stop_btn,
            run_row,
            menu_btns,
            panes,
            gitpanel,
            dirty: false,
            editor_state: EditorSnapshot::default(),
            running: false,
            editor,
            pending_commit: None,
            probe_ws: None,
            probe_s3: None,
        };

        register_actions(&relm4::main_application(), &root, sender.clone(), ed_in);
        model.refresh_menus();

        // window.py: win.connect("notify::is-active", lambda: tree.refresh_git())
        // — python fires on EVERY notify (both False→True and the rare True→False
        // edge); each fire costs one sync refresh — keep exactly that.
        let in_active = sender.clone();
        root.connect_notify_local(Some("is-active"), move |_, _| {
            in_active.input(AppMsg::WindowActive)
        });

        // startup workspace (window.py:154 — last_project populates everything)
                if let Some(p) = model.root.clone() {
            sender.input(AppMsg::SetWorkspace(PathBuf::from(p)));
        }
        // PROBE-ONLY (ALPACA_PROBE_S2) — never runs in production (env unset)
        if let Ok(dir) = std::env::var("ALPACA_PROBE_S2") {
            sender.input(AppMsg::ProbeS2(PathBuf::from(dir)));
        }
        // PROBE-ONLY (ALPACA_PROBE_S3) — never runs in production (env unset)
        if let Ok(dir) = std::env::var("ALPACA_PROBE_S3") {
            sender.input(AppMsg::ProbeS3(PathBuf::from(dir)));
        }

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match msg {
            AppMsg::SetWorkspace(p) => {
                self.set_workspace(&p, root, sender);
            }
            AppMsg::DiscardWorkspace(p) => {
                self.dirty = false;
                self.set_workspace_now(&p, root);
                self.refresh_menus();
            }
            AppMsg::OpenFile(p) => {
                // filetree → editor routing (tree lands in Task 7)
                self.editor.emit(EditorMsg::OpenFile(p));
            }
            AppMsg::EditorState(s) => {
                self.dirty = s.has_dirty;
                self.editor_state = s;
                if self.root.is_some() {
                    // debounced ≤1s; each event re-arms (stacked timers flush
                    // the LATEST snapshot; last event before quit may be lost —
                    // acceptable, window.py:204)
                    let sender = sender.clone();
                    glib::timeout_add(std::time::Duration::from_millis(1000), move || {
                        sender.input(AppMsg::FlushTabs);
                        glib::ControlFlow::Break
                    });
                }
            }
            AppMsg::FlushTabs => {
                if let Some(root) = &self.root {
                    let st = self.editor_state.clone();
                    state::save(&state::set_tabs(
                        state::load(),
                        root,
                        st.open_tabs,
                        st.active_tab,
                    ));
                }
            }
            AppMsg::RunStatus(running) => {
                // window.py:_on_run_status — the payload is not displayed; the
                // buttons + pill class follow the running mirror
                self.running = running;
                if !running {
                    self.run_btn.set_sensitive(true);
                }
                self.stop_btn.set_sensitive(running);
                self.refresh_run_style();
            }
            AppMsg::RunStarting => {
                self.running = true;
                self.run_btn.set_sensitive(false); // Stop-only until run ends
                self.refresh_run_style();
            }
            AppMsg::RunPressed => {
                // window.py:_on_run: no root / live run → noop
                if self.root.is_none() || self.running {
                    return;
                }
                let Some(cmd) = runctl::detect(Path::new(self.root.as_deref().unwrap())) else {
                    self.run_btn.set_sensitive(false);
                    self.run_btn
                        .set_tooltip_text(Some("No dev/start script or .csproj found"));
                    return;
                };
                self.stop_btn.set_sensitive(true);
                self.panes
                    .emit(PanesMsg::LaunchRun { argv: cmd.argv });
                // run-off + style land with RunStarting (accepted) — python's
                // _on_run does it synchronously; one mainloop turn, same frame
            }
            AppMsg::StopPressed => {
                // window.py:_on_stop — unconditional button sync, then the panes
                self.panes.emit(PanesMsg::StopRun);
                self.stop_btn.set_sensitive(false);
                self.run_btn.set_sensitive(true);
                self.refresh_run_style();
            }
            AppMsg::RecentMissing(p) => {
                error_dialog(root, &format!("Project not found: {p}"), "");
            }
            AppMsg::OpenDiff { rel, letter } => {
                // window.py:_open_changes_diff runs ON the window with its own
                // root guard — same shape here (App's root is Option<String>)
                if self.root.is_none() {
                    return;
                }
                let root_path = PathBuf::from(self.root.as_deref().unwrap().to_owned());
                self.editor.emit(EditorMsg::OpenDiff { root: root_path, rel, letter });
            }
            AppMsg::Commit { root, paths, msg } => {
                // python gitview.commit_clicked called before_commit(paths)
                // SYNCHRONOUSLY (editor.save_open on the same mainloop turn) —
                // relm4 inputs are QUEUE-processed, not call-stack: the turn's
                // equivalent is the pending store + the Flushed ack chain
                self.pending_commit = Some((root.clone(), paths.clone(), msg.clone()));
                self.editor.emit(EditorMsg::SaveOpen(
                    paths.iter().map(|rel| root.join(rel)).collect(),
                ));
            }
            AppMsg::Flushed => {
                // the editor's Flushed strictly follows its SaveOpen in its
                // FIFO input queue → a sound chain (T5's_save_open ALWAYS
                // emits Flushed, zero-flushed included — the chain cannot
                // strand; eager-busy closes the duplicate-click window)
                if let Some((root, paths, msg)) = self.pending_commit.take() {
                    self.gitpanel.emit(GitPanelMsg::StartCommit { root, paths, msg });
                }
            }
            AppMsg::GitChanged(rows) => {
                // window.py:_on_git_changed — diff-tab renewal. A stale diff
                // tab across workspaces: rust restore (S1) rebuilds ONLY path
                // pages, so a diff page stays open with stale sides — same as
                // python (measured: _set_workspace_now re-roots the trees, the
                // letters map misses the foreign rel, fresh is None → left
                // as-is). Self.root None → no diffs can exist (skip).
                if self.root.is_none() {
                    return;
                }
                let root_path = PathBuf::from(self.root.as_deref().unwrap().to_owned());
                let letters = rows.iter().map(|(rel, letter)| (rel.clone(), letter.clone())).collect();
                self.editor.emit(EditorMsg::RefreshDiffs { root: root_path, letters });
            }
            AppMsg::WindowActive => {
                // filetree.py:285-287 refresh_git — window focus re-syncs git
                self.gitpanel.emit(GitPanelMsg::RefreshGit);
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
            AppMsg::ProbeS2Step => {
                // PROBE-ONLY (ALPACA_PROBE_S2) — never runs in production
                // (env unset); one chain, each timer BREAKs — no re-arm hazard
                self.probe_step(&sender);
            }
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
                // (env unset); one chain, each timer BREAKs — no re-arm hazard
                let Some((ws, step)) = self.probe_s3.as_ref().map(|(w, s)| (w.clone(), *s)) else {
                    return;
                };
                println!("PROBE step {}: ts={}", step, epoch_ms());
                // the window holds TWO widget children (titlebar wrapper first,
                // hpane second — set_titlebar precedes set_child) so first_child
                // alone lands on the WindowHandle; walk every child
                let hpane = {
                    let mut found: Option<gtk::Paned> = None;
                    let mut c = root.first_child();
                    while let Some(w) = c {
                        let next = w.next_sibling();
                        if let Some(p) = w.downcast::<gtk::Paned>().ok() {
                            found = Some(p);
                        }
                        c = next;
                    }
                    found
                };
                match step {
                    0 => {
                        sender.input(AppMsg::SetWorkspace(ws.clone()));
                        for f in ["package-lock.json", "next.config.ts", "components.json",
                                  "related-products.tsx", "trude.json",
                                  "deep/path/segments/segment-file-with-long-name.tsx"] {
                            sender.input(AppMsg::OpenFile(ws.join(f)));
                        }
                    }
                    1 => {
                        let checks = [
                            ("window", root.measure(gtk::Orientation::Horizontal, -1).0, 560),
                            ("editor", self.editor.widget().measure(gtk::Orientation::Horizontal, -1).0, 300),
                            ("panes", self.panes.widget().measure(gtk::Orientation::Horizontal, -1).0, 300),
                        ];
                        for (name, got, cap) in checks {
                            let ok = got <= cap;
                            println!("PROBE assert: s3-min-{name} {got}<={cap}: {}",
                                     if ok { "PASS" } else { "FAIL" });
                        }
                        self.gitpanel.emit(GitPanelMsg::TreeMin);
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
                    3 => if let Some(h) = &hpane { h.set_position(900); },
                    4 => {
                        if let Some(h) = &hpane {
                            println!("PROBE hpane pos={} w={} at wide-trace",
                                     h.position(), h.allocated_width());
                        }
                        self.editor.emit(EditorMsg::ProbeTrace("wide".into()));
                    }
                    5 => if let Some(h) = &hpane { h.set_position(850); },
                    6 => {
                        self.editor.emit(EditorMsg::ProbeTrace("narrow".into()));
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
    /// drives the step at `probe_ws`'s index, then schedules the next one.
    fn probe_step(&mut self, sender: &ComponentSender<Self>) {
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
            // the REAL workspace route — it feeds panel SetRoot + editor
            // Restore + last_project; the OpenDiff arm needs App root set
            0 => sender.input(AppMsg::SetWorkspace(ws.clone())),
            1 => self.gitpanel.emit(GitPanelMsg::ModeChanges),
            // commit_clicked rejects an empty entry — fill the real entry
            2 => self.gitpanel.emit(GitPanelMsg::ProbeChild(ProbeChild::Changes(
                GitViewMsg::ProbeMsg(PROBE_MSG.into()),
            ))),
            // the diff page for the fix-3 assert (Edit-menu target on a diff
            // page must be the focused side view, python _edit_op)
            3 => sender.input(AppMsg::OpenDiff {
                rel: "a.txt".into(),
                letter: "M".into(),
            }),
            4 => self.editor.emit(EditorMsg::ProbeClip),
            // typing steps — fill the real search entry; GtkSearchEntry's
            // delayed "search-changed" fires the REAL EntryChanged route.
            // Pre-fix the changes arm re-fetches (SYNC git per keystroke);
            // its stall straddles the next typing step's timer — deltas ≈
            // 150ms delay + shim sleep (green ≈ the 400ms gap).
            5 => self.gitpanel.emit(GitPanelMsg::ProbeEntry("alpha".into())),
            6 => self.gitpanel.emit(GitPanelMsg::ProbeEntry("alphabet".into())),
            7 => self.gitpanel.emit(GitPanelMsg::ProbeEntry("alphabetZ".into())),
            8 => self.gitpanel.emit(GitPanelMsg::ProbeEntry("alphabetZx".into())),
            // clear the needle so the masthead sees the full set again
            9 => self.gitpanel.emit(GitPanelMsg::ProbeEntry("".into())),
            // select-all via the REAL row-toggle law — but rows arrive
            // PRE-CHECKED (python's "fresh view selects all", gitview SetRoot
            // → refresh(false)): one masthead toggle would DESELECT. Toggle
            // twice (off→on) so the live toggle path runs and the state ends
            // all-checked, python-exact.
            10 => {
                let mast = || GitPanelMsg::ProbeChild(ProbeChild::Changes(
                    GitViewMsg::RowToggled(gtk::TreePath::from_string("0").unwrap()),
                ));
                self.gitpanel.emit(mast()); // off
                self.gitpanel.emit(mast()); // on — ends python's all-checked
            }
            // the FULL production chain: validate → eager busy → Commit out
            // → App Commit → SaveOpen → Flushed → panel StartCommit → flight
            11 => self.gitpanel.emit(GitPanelMsg::ProbeChild(ProbeChild::Changes(
                GitViewMsg::CommitClicked,
            ))),
            // MID-FLIGHT injection: wsB is its own clean repo; the flight's
            // root was SNAPSHOT into StartCommit's payload, so the commit
            // still lands on wsA and wsB receives nothing (asserted next)
            12 => sender.input(AppMsg::SetWorkspace(PathBuf::from(&ws_b))),
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
        self.probe_ws.as_mut().unwrap().1 = step + 1;
        let ms = step_ms(step + 1);
        let snd = sender.clone();
        glib::timeout_add(std::time::Duration::from_millis(ms), move || {
            snd.input(AppMsg::ProbeS2Step);
            glib::ControlFlow::Break
        });
    }

    fn set_workspace(&mut self, path: &Path, root: &gtk::ApplicationWindow, sender: ComponentSender<Self>) {
        if self.dirty {
            self.show_discard_dialog(root, path, sender);
            return;
        }
        self.set_workspace_now(path, root);
        // refresh recents after any workspace switch (python refreshes in
        // _on_pick/_act_new_project; all of those land here)
        self.refresh_menus();
    }

    /// window.py:set_workspace's discard flow: CANCEL/Discard on the MessageDialog.
    fn show_discard_dialog(&self, root: &gtk::ApplicationWindow, path: &Path, sender: ComponentSender<Self>) {
        #[allow(deprecated)] // 4.10-deprecated upstream; window.py uses it (S3 polish)
        let d = gtk::MessageDialog::builder()
            .transient_for(root)
            .modal(true)
            .text("Discard unsaved changes?")
            .buttons(gtk::ButtonsType::Cancel)
            .build();
        d.add_button("Discard", gtk::ResponseType::Accept);
        let path = path.to_path_buf();
        d.connect_response(move |dd, r| {
            dd.destroy(); // python always destroys in the response handler
            if r == gtk::ResponseType::Accept {
                sender.input(AppMsg::DiscardWorkspace(path.clone()));
            }
        });
        d.present();
    }

    fn set_workspace_now(&mut self, path: &Path, root: &gtk::ApplicationWindow) {
        let path_str = path.to_string_lossy();
        state::save(&state::remember(state::load(), path_str.as_ref()));
        self.root = Some(path_str.to_string());
        root.set_title(Some("alpaca_code"));
        let base = py_basename(&path_str);
        self.title.set_text(&if base.is_empty() {
            "alpaca-code".to_owned()
        } else {
            format!("alpaca-code — {base}")
        });
        // tree first, then panes, like window.py:_set_workspace_now;
        // notify::is-active → refresh_git comes true in T7
        self.gitpanel.emit(GitPanelMsg::SetRoot(path.to_path_buf()));
        self.panes.emit(PanesMsg::SetRoot(path.to_path_buf()));
        let cmd = runctl::detect(path);
        self.run_btn.set_sensitive(cmd.is_some());
        self.run_btn.set_tooltip_text(Some(
            cmd.as_ref()
                .map(|c| c.label.as_str())
                .unwrap_or("No dev/start script or .csproj found"),
        ));
        self.stop_btn.set_sensitive(false);
        self.refresh_run_style();
        // window.py:_set_workspace_now — editor.restore LAST (python order)
        let tabs = state::project_tabs(&state::load(), path_str.as_ref());
        self.editor
            .emit(EditorMsg::Restore(path.to_path_buf(), tabs.open_tabs, tabs.active_tab));
    }

    fn refresh_menus(&self) {
        refresh_menus(&self.menu_btns);
    }

    /// window.py:_refresh_run_style — the running class rides the pill row
    /// (App tracks `running` off the panes' outputs; python read it live).
    fn refresh_run_style(&self) {
        if self.running {
            self.run_row.set_css_classes(&["alpaca-runpill", "running"]);
        } else {
            self.run_row.set_css_classes(&["alpaca-runpill"]);
        }
    }
}

// window.py basename(path.rstrip("/")) — empty result (root "/") → bare title
fn py_basename(s: &str) -> String {
    let t = s.trim_end_matches('/');
    t.rsplit('/').find(|c| !c.is_empty()).unwrap_or(t).to_owned()
}

/// window.py:37-46 — 1586×992 clamped to the monitor (−40 top/side pad, −140
/// vertical incl. panel); a too-tall default opens half-offscreen. ponytail: monitor 0.
fn default_size() -> (i32, i32) {
    if let Some(disp) = gdk::Display::default() {
        let mons = disp.monitors();
        if mons.n_items() > 0 {
            if let Some(mon) = mons.item(0).and_downcast::<gdk::Monitor>() {
                let g = mon.geometry();
                return (1586.min(g.width() - 40), 992.min(g.height() - 140));
            }
        }
    }
    (1586, 992)
}

/// Label-mode MenuButtons draw a real 16×24 `down` arrow widget (invisible
/// glyph, real width → menu labels sat 38px apart). set_always_show_arrow(False)
/// no-ops on this build; hide the widget instead; survives set_popover (probed).
fn no_arrow(btn: &gtk::MenuButton) {
    let c = match btn.first_child().and_then(|b| b.first_child()) {
        Some(c) => c,
        None => return,
    };
    let mut c = c;
    loop {
        if c.has_css_class("down") {
            c.set_visible(false);
            return;
        }
        match c.next_sibling() {
            Some(sib) => c = sib,
            None => return,
        }
    }
}

fn refresh_menus(btns: &HeaderButtons) {
    // File menu lives in exactly one place: the headerbar File button.
    let s = state::load();
    btns.file.set_popover(Some(&gtk::PopoverMenu::from_model(Some(&file_menu(&s)))));
    btns.edit.set_popover(Some(&gtk::PopoverMenu::from_model(Some(&edit_menu()))));
    btns.window.set_popover(Some(&gtk::PopoverMenu::from_model(Some(&win_menu()))));
    btns.help.set_popover(Some(&gtk::PopoverMenu::from_model(Some(&help_menu()))));
}

fn file_menu(s: &state::State) -> gio::Menu {
    let menu = gio::Menu::new();
    menu.append(Some("Open Project…"), Some("app.open-project"));
    // recents submenu: "basename(rstrip /) — path" rows, newest first; the
    // submenu row only exists when there is at least one recent (python
    // window.py:383 `if recents.get_n_items()`)
    let recents = gio::Menu::new();
    for p in &s.recents {
        let item = gio::MenuItem::new(Some(&format!("{} — {}", py_basename(p), p)), None);
        item.set_action_and_target_value(Some("app.open-recent"), Some(&glib::Variant::from(p)));
        recents.append_item(&item);
    }
    if recents.n_items() > 0 {
        menu.append_submenu(Some("Open Recent"), &recents);
    }
    menu.append(Some("New Project…"), Some("app.new-project"));
    menu.append(Some("Quit"), Some("app.quit"));
    menu
}

fn edit_menu() -> gio::Menu {
    let m = gio::Menu::new();
    let sec = gio::Menu::new();
    sec.append(Some("Undo"), Some("app.undo"));
    sec.append(Some("Redo"), Some("app.redo"));
    m.append_section(None, &sec);
    let sec = gio::Menu::new();
    sec.append(Some("Cut"), Some("app.cut"));
    sec.append(Some("Copy"), Some("app.copy"));
    sec.append(Some("Paste"), Some("app.paste"));
    m.append_section(None, &sec);
    let sec = gio::Menu::new();
    sec.append(Some("Select All"), Some("app.select-all"));
    m.append_section(None, &sec);
    m
}

fn win_menu() -> gio::Menu {
    let m = gio::Menu::new();
    m.append(Some("Minimize"), Some("app.win-minimize"));
    m.append(Some("Close"), Some("app.close"));
    m
}

fn help_menu() -> gio::Menu {
    let m = gio::Menu::new();
    m.append(Some("About alpaca-code"), Some("app.about"));
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
        if let Some(p) = path {
            if p.is_dir() {
                sender.input(AppMsg::SetWorkspace(p));
            }
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
    let root_win = root_win.clone();
    d.connect_response(move |dd, resp| {
        let name = name_e.text().trim().to_owned();
        let parent = parent_e.text().trim().to_owned();
        dd.destroy();
        if resp != gtk::ResponseType::Accept || name.is_empty() {
            return;
        }
        let target = Path::new(&parent).join(name.as_str());
        match runctl::create_project(&target) {
            Err(e) => error_dialog(&root_win, &format!("Couldn't create {}", target.display()), &e),
            Ok(()) => sender.input(AppMsg::SetWorkspace(target)),
        }
    });
    d.present();
}

/// window.py:_error_dialog — MessageDialog CLOSE destroy-on-response.
fn error_dialog(root: &gtk::ApplicationWindow, primary: &str, secondary: &str) {
    show_message(root, primary, Some(secondary), gtk::ButtonsType::Close);
}

/// MessageDialog is 4.10-deprecated upstream; window.py uses it throughout
/// (parity wins, AlertDialog redesign is S3 polish). python never passed a
/// message_type → INFO default; secondary set post-hoc like the constructor kwarg.
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
    ed_in: relm4::Sender<EditorMsg>,
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

    // window.py:_edit_op — undo/redo buffer-side, clipboard ops on the view's
    // own actions; S1 has no diff pages so there's no pair-box edge
    for name in ["undo", "redo", "cut", "copy", "paste", "select-all"] {
        let a = gio::SimpleAction::new(name, None);
        let ed = ed_in.clone();
        a.connect_activate(move |_a, _p| {
            let msg = match name {
                "undo" => EditorMsg::Undo,
                "redo" => EditorMsg::Redo,
                "cut" => EditorMsg::Cut,
                "copy" => EditorMsg::Copy,
                "paste" => EditorMsg::Paste,
                _ => EditorMsg::SelectAll,
            };
            ed.send(msg).unwrap();
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
                sender.input(AppMsg::SetWorkspace(p.into()));
            } else {
                sender.input(AppMsg::RecentMissing(p));
            }
        });
    }
    app.add_action(&recents);

    app.set_accels_for_action("app.quit", &["<Control>q"]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::State;

    fn fixture(recents: &[&str]) -> State {
        State {
            last_project: None,
            recents: recents.iter().map(|s| s.to_string()).collect(),
            projects: Default::default(),
        }
    }

    /// MenuModel item's attribute as a String (label / action).
    fn attr(model: &impl glib::object::IsA<gio::MenuModel>, name: &str, i: i32) -> Option<String> {
        use gio::prelude::MenuModelExt;
        model
            .item_attribute_value(i, name, None)
            .and_then(|v| v.get::<String>())
    }

    #[test]
    fn file_menu_items_and_recents_submenu_model() {
        use gio::prelude::MenuModelExt;
        // window.py:_build_menu — fixed rows + recents submenu (newest first,
        // each "basename(rstrip) — path"), submenu only when recents non-empty.
        let m = file_menu(&fixture(&["/home/u/misiuscode", "/home/u/porttest"]));
        assert_eq!(m.n_items(), 4);
        assert_eq!(attr(&m, "label", 0).as_deref(), Some("Open Project…"));
        assert_eq!(attr(&m, "label", 2).as_deref(), Some("New Project…"));
        assert_eq!(attr(&m, "label", 3).as_deref(), Some("Quit"));
        let rec = m
            .item_link(1, gio::MENU_LINK_SUBMENU)
            .expect("item 1 must be the Open Recent submenu");
        assert_eq!(rec.n_items(), 2);
        assert_eq!(
            attr(&rec, "label", 0).as_deref(),
            Some("misiuscode — /home/u/misiuscode")
        );
        assert_eq!(
            attr(&rec, "label", 1).as_deref(),
            Some("porttest — /home/u/porttest")
        );
        // action wiring: item 0 → open-project, Quit → app.quit
        assert_eq!(attr(&m, "action", 0).as_deref(), Some("app.open-project"));
        assert_eq!(attr(&m, "action", 1), None); // submenu item carries no action
        assert_eq!(attr(&m, "action", 2).as_deref(), Some("app.new-project"));
        assert_eq!(attr(&m, "action", 3).as_deref(), Some("app.quit"));
    }

    #[test]
    fn file_menu_without_recents_has_no_submenu() {
        use gio::prelude::MenuModelExt;
        // python: `if recents.get_n_items(): menu.append_submenu(...)`
        let m = file_menu(&fixture(&[]));
        assert_eq!(m.n_items(), 3);
        assert!(
            m.item_link(1, gio::MENU_LINK_SUBMENU).is_none(),
            "empty recents must not leave an empty submenu row"
        );
        assert_eq!(attr(&m, "label", 1).as_deref(), Some("New Project…"));
        assert_eq!(attr(&m, "label", 2).as_deref(), Some("Quit"));
    }

    #[test]
    fn edit_window_help_menus_populated() {
        use gio::prelude::MenuModelExt;
        // brief step 4: "Edit/Window/Help populated" — sections 2/3/1, Win 2, Help 1
        let e = edit_menu();
        assert_eq!(e.n_items(), 3); // 3 sections
        for (i, n) in [(0, 2), (1, 3), (2, 1)] {
            let sec = e
                .item_link(i, gio::MENU_LINK_SECTION)
                .expect("section");
            assert_eq!(sec.n_items(), n, "section {i}");
        }
        assert_eq!(win_menu().n_items(), 2);
        assert_eq!(help_menu().n_items(), 1);
    }
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