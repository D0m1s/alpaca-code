//! GitPanel card (python filetree.py:29-373 — FileBrowser with git modes):
//! the file tree, the changes view, the branch pill and a status row behind
//! one chrome. The tree's chrome was trimmed here (Task 6); this file owns
//! head/search/mode-tabs/stack/status-row and the git status plumbing.

use std::path::PathBuf;
use std::time::Duration;

use relm4::gtk::{self, gio, glib, pango, prelude::*};
use relm4::prelude::*;

use crate::branchmenu::{BranchMenu, BranchMenuMsg, BranchMenuOutput};
use crate::filetree::{FileTree, FileTreeMsg, FileTreeOutput};
use crate::gitstatus::{self, GitStatusKind, Rows};
use crate::gitview::{ChangesView, GitViewMsg, GitViewOutput};

/// Probe-only child driver: T8's probe sends child inputs through the panel so
/// probe-driven flights behave exactly like real ones (child outputs still
/// route through the panel's receivers). Production never constructs it.
/// NOT Clone (FileTreeMsg carries TreePath) — the probe sends each value once.
#[derive(Debug)]
pub enum ProbeChild {
    Changes(GitViewMsg),
}

#[derive(Debug)]
pub enum GitPanelMsg {
    SetRoot(PathBuf),
    ModeTree,
    ModeChanges,
    EntryChanged(String),
    /// gitview + branchmenu status reports land here (window wiring's target)
    ChildStatus(GitStatusKind, String),
    ChildBusy(bool),
    Count(usize),
    /// the 2s probe timer's single recurring tick
    ProbeTick,
    /// The ok pulse's timeout fires once per generation token (the gen IS the
    /// token); a PulseFired for a superseded gen is discarded.
    PulseFired(u64),
    /// HEAD monitor + window focus hook (T7 wires the focus arm)
    RefreshGit,
    /// T7's flush chain: editor Flushed → App → StartCommit relay
    StartCommit {
        root: PathBuf,
        paths: Vec<String>,
        msg: String,
    },
    /// probe-only child proxy — production never sends it
    ProbeChild(ProbeChild),
    /// probe-only: fill the real search entry so its (delayed) search-changed
    /// signal fires the REAL EntryChanged route; production never sends it
    ProbeEntry(String),
    /// PROBE-ONLY (ALPACA_PROBE_S3) — filetree min-width assert; production
    /// never sends it
    TreeMin,
}

#[derive(Debug)]
pub enum GitPanelOutput {
    OpenFile(PathBuf),
    OpenDiff {
        rel: String,
        letter: String,
    },
    Commit {
        root: PathBuf,
        paths: Vec<String>,
        msg: String,
    },
    /// T7 consumes: diff-tab hook (editor RefreshDiffs)
    GitChanged(Rows),
}

#[derive(Debug)]
pub enum GitPanelCommand {
    ProbeLanded {
        root: String,
        raw: Option<Rows>,
        ahead: usize,
    },
}

pub struct GitPanel {
    root: Option<PathBuf>,
    /// the card mode — "tree" | "changes"
    mode: &'static str,
    /// a commit/push flight owns the row (python _git_busy)
    git_busy: bool,
    probe_busy: bool,
    changes_busy: bool,
    /// the status pulse's generation counter (python _pulse_id); SourceId::
    /// remove is forbidden (timer law) and cancel happens BY GENERATION
    pulse_gen: u64,
    head_mon: Option<gio::FileMonitor>,
    entry: gtk::SearchEntry,
    dot: gtk::Box,
    spin: gtk::Spinner,
    git_label: gtk::Label,
    count_label: gtk::Label,
    ws_btn: gtk::Button,
    ch_btn: gtk::Button,
    stack: gtk::Stack,
    // stored controllers — T8's probe drives child inputs through them
    ft: relm4::Controller<FileTree>,
    changes: relm4::Controller<ChangesView>,
    branch: relm4::Controller<BranchMenu>,
}

#[relm4::component(pub)]
impl Component for GitPanel {
    type CommandOutput = GitPanelCommand;
    type Input = GitPanelMsg;
    type Output = GitPanelOutput;
    type Init = ();

    view! {
        #[root]
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            set_css_classes: &["alpaca-card"], // card css MOVED here from filetree (block 11)
            set_overflow: gtk::Overflow::Hidden, // clip children to the card's rounded corners
        }
    }

    fn init(_: (), root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        // ---- child components FIRST (their widgets feed the assembly below)
        // each receiver routes the child's outputs through the panel as real
        // inputs/outputs — the same path real flights take (T8's ProbeChild
        // rides it too)
        let snd_ft = sender.clone();
        let ft = FileTree::builder().launch(()).connect_receiver(move |_, out| match out {
            FileTreeOutput::OpenFile(p) => {
                let _ = snd_ft.output(GitPanelOutput::OpenFile(p));
            }
            FileTreeOutput::Count(n) => snd_ft.input(GitPanelMsg::Count(n)),
        });
        let snd_ch = sender.clone();
        let changes = ChangesView::builder()
            .launch(())
            .connect_receiver(move |_, out| match out {
                GitViewOutput::Open { rel, letter } => {
                    let _ = snd_ch.output(GitPanelOutput::OpenDiff { rel, letter });
                }
                GitViewOutput::Commit { root, paths, msg } => {
                    let _ = snd_ch.output(GitPanelOutput::Commit { root, paths, msg });
                }
                GitViewOutput::Status(kind, text) => snd_ch.input(GitPanelMsg::ChildStatus(kind, text)),
                GitViewOutput::Busy(b) => snd_ch.input(GitPanelMsg::ChildBusy(b)),
            });
        let snd_br = sender.clone();
        let branch = BranchMenu::builder().launch(()).connect_receiver(move |_, out| match out {
            BranchMenuOutput::Status(kind, text) => snd_br.input(GitPanelMsg::ChildStatus(kind, text)),
        });

        // ---- card chrome (python 89-195 values verbatim)
        let head = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(4)
            .margin_start(14)
            .margin_end(10)
            .margin_top(8)
            .margin_bottom(6)
            .build();
        head.append(
            &gtk::Label::builder()
                .label("File Browser")
                .xalign(0.0)
                .hexpand(true)
                .css_classes(["alpaca-panel-title"])
                .build(),
        );

        let entry = gtk::SearchEntry::builder()
            .placeholder_text("Search files…")
            .css_classes(["alpaca-search"])
            .hexpand(false)
            .vexpand(false)
            .halign(gtk::Align::Fill)
            .margin_start(12)
            .margin_end(12)
            .margin_bottom(6)
            .build();
        // 30px floor: the entry renders ~36 (Breeze searchentry min, swept to
        // the floor in main.py CSS) — the request only keeps it honest below that
        entry.set_size_request(-1, 30);
        let snd_eg = sender.clone();
        entry.connect_search_changed(move |e| {
            snd_eg.input(GitPanelMsg::EntryChanged(e.text().to_string()));
        });

        // WORKSPACE / CHANGES mode tabs (spec §1): text-only section-label type.
        // has_frame(false) + the transparent .alpaca-tabbtn rule keep them bare
        // text; set_mode moves the `alpaca-on` class as the selection. Full
        // width: the hairline under this strip is the box's own border-bottom —
        // margins sit OUTSIDE the border box (measured breadcrumb gotcha), so
        // the inset is padding and margin_top only (python comment verbatim).
        let btns = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(10)
            .margin_top(6)
            .hexpand(true)
            .css_classes(["alpaca-modetabs"])
            .build();
        let ws_btn = gtk::Button::builder()
            .label("WORKSPACE")
            .has_frame(false)
            .css_classes(["alpaca-tabbtn", "alpaca-on"]) // tree mode on by default
            .build();
        let ch_btn = gtk::Button::builder()
            .label("CHANGES")
            .has_frame(false)
            .css_classes(["alpaca-tabbtn"])
            .build();
        ch_btn.set_visible(false); // outside-repo policy (§2); _refresh_status decides
        let snd_ws = sender.clone();
        ws_btn.connect_clicked(move |_| snd_ws.input(GitPanelMsg::ModeTree));
        let snd_cm = sender.clone();
        ch_btn.connect_clicked(move |_| snd_cm.input(GitPanelMsg::ModeChanges));
        btns.append(&ws_btn);
        btns.append(&ch_btn);

        // mode host (§1): zero-transition swap between the tree and the changes
        // view. vexpand on the Stack — GtkBox gives non-expanding children only
        // their minimum, and the card must fill below the tab row (python
        // comment verbatim)
        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::None)
            .vexpand(true)
            .build();
        stack.add_named(ft.widget(), Some("tree"));
        stack.add_named(changes.widget(), Some("changes"));
        stack.set_visible_child_name("tree");

        let bar = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .margin_start(14)
            .margin_end(12)
            .margin_top(0)
            .margin_bottom(0)
            .css_classes(["alpaca-statusbar"])
            .build();
        bar.append(branch.widget()); // python appends branchbtn FIRST
        let dot = gtk::Box::builder()
            .css_classes(["alpaca-status-dot"])
            .valign(gtk::Align::Center)
            .build();
        dot.set_size_request(8, 8);
        dot.set_visible(false);
        let spin = gtk::Spinner::builder()
            .css_classes(["alpaca-status-spin"])
            .valign(gtk::Align::Center)
            .build();
        spin.set_size_request(10, 10);
        spin.set_visible(false);
        let git_label = gtk::Label::builder()
            .label("")
            .ellipsize(pango::EllipsizeMode::Middle)
            .build();
        git_label.set_visible(false);
        let count_label = gtk::Label::new(None);
        let spacer = gtk::Box::builder().hexpand(true).build(); // spacer pushes file count right
        bar.append(&dot);
        bar.append(&spin);
        bar.append(&git_label);
        bar.append(&spacer);
        bar.append(&count_label);

        root.append(&head);
        root.append(&entry);
        root.append(&btns);
        root.append(&stack);
        root.append(&bar);

        // ---- live git probe (python _git_tick recurrence; timer law — ms form)
        // every 2s the porcelain status runs in a worker thread and lands on
        // the UI thread — covers agent edits anywhere in the tree (dir monitors
        // only see dirs we've opened). Skipped while a flight owns the row or a
        // probe/commit is busy.
        let snd_tick = sender.clone();
        // LOCAL: the closure is main-thread-only and does NOT need Send
        // (GitPanelMsg contains TreePath via ProbeChild — unsafe for a
        // cross-thread Sender; the local sources dodge the bound entirely)
        glib::timeout_add_local(Duration::from_millis(2000), move || {
            snd_tick.input(GitPanelMsg::ProbeTick);
            glib::ControlFlow::Continue
        });

        let widgets = view_output!();
        let model = GitPanel {
            root: None,
            mode: "tree",
            git_busy: false,
            probe_busy: false,
            changes_busy: false,
            pulse_gen: 0,
            head_mon: None,
            entry,
            dot,
            spin,
            git_label,
            count_label,
            ws_btn,
            ch_btn,
            stack,
            ft,
            changes,
            branch,
        };

        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: GitPanelMsg, sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            GitPanelMsg::SetRoot(path) => self.set_root(path, &sender),
            GitPanelMsg::ModeTree => self.set_mode("tree"),
            GitPanelMsg::ModeChanges => self.set_mode("changes"),
            GitPanelMsg::EntryChanged(text) => self.entry_changed(&text),
            GitPanelMsg::ChildStatus(kind, text) => self.show_git_status(kind, text, &sender),
            GitPanelMsg::ChildBusy(b) => self.changes_busy = b,
            GitPanelMsg::Count(n) => self.count_label.set_text(&format!("{n} files")),
            GitPanelMsg::ProbeTick => self.probe_tick(&sender),
            GitPanelMsg::PulseFired(g) => {
                if g == self.pulse_gen {
                    // End of the ok pulse: stop owning the row, re-sync to the
                    // real state (python _pulse; the gen check discards stale ids)
                    self.refresh_status(&sender);
                }
            }
            GitPanelMsg::RefreshGit => self.refresh_status(&sender),
            GitPanelMsg::StartCommit { root, paths, msg } => {
                self.changes.emit(GitViewMsg::StartCommit { root, paths, msg }); // T7's flush chain lands here
            }
            GitPanelMsg::ProbeChild(pc) => match pc {
                ProbeChild::Changes(m) => self.changes.emit(m),
            },
            // probe-only: typing through the real entry — its delayed
            // search-changed fires the same EntryChanged route a user's
            // keystroke does
            GitPanelMsg::ProbeEntry(t) => self.entry.set_text(&t),
            GitPanelMsg::TreeMin => {
                let min = self.ft.widget().measure(gtk::Orientation::Horizontal, -1).0;
                let ok = min <= 170;
                println!("PROBE assert: s3-min-tree: {}", if ok { "PASS" } else { "FAIL" });
            }
        }
    }

    fn update_cmd(&mut self, cmd: GitPanelCommand, sender: ComponentSender<Self>, _root: &Self::Root) {
        match cmd {
            GitPanelCommand::ProbeLanded { root, raw, ahead } => {
                self.probe_busy = false;
                // python _git_landed guard — same three terms; root compares as
                // String vs Option: map the stored path first
                let cur = self.root.as_ref().map(|p| p.to_string_lossy().into_owned());
                if cur.as_deref() != Some(root.as_str()) || self.git_busy || self.changes_busy {
                    return;
                }
                self.paint_status(gitstatus::branch_of(&root).as_deref(), raw.as_ref().map(|r| r.len()));
                // python 351-355 order: paint, then the hook, then the open view
                if let Some(raw) = raw {
                    let _ = sender.output(GitPanelOutput::GitChanged(raw.clone()));
                    if self.mode == "changes" {
                        self.changes.emit(GitViewMsg::Apply { raw: Some(raw), ahead, keep: true });
                    }
                }
            }
        }
    }
}

// ---- git status plumbing (python 197-356) --------------------------------

impl GitPanel {
    /// python set_root (198-215) step for step.
    fn set_root(&mut self, root: PathBuf, sender: &ComponentSender<Self>) {
        if let Some(m) = self.head_mon.take() {
            m.cancel(); // python cancels its monitors at set_root top
        }
        self.root = Some(root.clone());
        // fires EntryChanged("") queued — python RELIES on that same fire (its
        // search-changed ran synchronously; the filetree's own queued Search("")
        // repeats are python parity — see its set_root comment)
        self.entry.set_text("");
        self.ft.emit(FileTreeMsg::SetRoot(root.clone())); // populate + count
        self.set_mode("tree"); // a fresh workspace opens on the file tree (spec §1)
        self.changes.emit(GitViewMsg::SetRoot(root.clone()));
        // python's branchmenu root attr was NEVER fed (latent bug in
        // branchmenu.py) — T4's fix lands here
        self.branch.emit(BranchMenuMsg::SetRoot(Some(root.clone())));
        let head = root.join(".git").join("HEAD");
        if head.is_file() {
            // python _head_mon's changed → _refresh_status; Rust's single
            // RefreshGit input covers monitor + focus (T7 wires the focus arm)
            // editor's watcher law: absent/unwatchable → no monitor; the 2s
            // probe keeps the status honest (python would traceback — degrade)
            if let Ok(mon) = gio::File::for_path(&head)
                .monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
            {
                let snd = sender.clone();
                mon.connect_changed(move |_, _, _, _| snd.input(GitPanelMsg::RefreshGit));
                self.head_mon = Some(mon);
            }
        }
        self.refresh_status(sender);
    }

    /// python _on_search routing: tree mode filters the tree; changes mode
    /// applies the needle to the EXISTING rows — python filetree.py:_on_search
    /// routes changes.filter(text) only, no re-fetch; the mode-entry refresh
    /// lives in set_mode (review fix-2: a `git status` + `rev-list` sync per
    /// keystroke on the UI thread is a python-behavioral deviation).
    fn entry_changed(&mut self, text: &str) {
        if self.mode == "tree" {
            self.ft.emit(FileTreeMsg::Search(text.to_string()));
        } else {
            self.changes.emit(GitViewMsg::Filter(text.to_string()));
        }
    }

    /// python _git_tick gate + probe thread (flight here).
    fn probe_tick(&mut self, sender: &ComponentSender<Self>) {
        if self.probe_busy || self.git_busy || self.changes_busy {
            return;
        }
        let Some(root) = self.root.as_ref().map(|p| p.to_string_lossy().into_owned()) else {
            return;
        };
        self.probe_busy = true;
        sender.spawn_command(move |out| {
            // probe-only fix-1 assert instrument: the wrapper times these
            // lines — a landed commit must NOT keep this flight from firing
            if std::env::var("ALPACA_PROBE_S2").is_ok() {
                println!("PROBE probe-flight: root={root} ts={}", crate::app::epoch_ms());
            }
            let raw = gitstatus::changes(&root); // snapshot: a workspace switch mid-flight drops it (guard in ProbeLanded)
            let ahead = if raw.is_some() { gitstatus::ahead(&root) } else { 0 };
            out.emit(GitPanelCommand::ProbeLanded { root, raw, ahead });
        });
    }

    /// python show_git_status (221-247): busy → spinner + phase text; ok →
    /// green dot pulse, the row re-syncs itself after 2s; err → red dot + the
    /// error's first line, full git output as a tooltip. The busy guard keeps
    /// mid-flight refreshes (HEAD monitor, mode switches) from clobbering the
    /// spinner.
    fn show_git_status(&mut self, kind: GitStatusKind, text: String, sender: &ComponentSender<Self>) {
        self.pulse_gen += 1; // cancel BY GENERATION: a fresh state outranks a stale pulse
        let g = self.pulse_gen;
        self.git_busy = kind == GitStatusKind::Busy;
        // python's allow = lambda: not self._git_busy re-evaluated per click —
        // the rust port pushes the value on every status change (busy included,
        // or the gate would stay open mid-flight)
        self.branch.emit(BranchMenuMsg::Gate(!self.git_busy));
        if kind == GitStatusKind::Busy {
            self.spin.set_visible(true);
            self.spin.start();
            self.dot.set_visible(false);
            self.git_label.set_text(&text);
            self.git_label.set_tooltip_text(Some(""));
            return;
        }
        self.spin.stop();
        self.spin.set_visible(false);
        self.dot.set_visible(true);
        self.git_label.set_visible(true);
        match kind {
            GitStatusKind::Ok => {
                self.dot.set_css_classes(&["alpaca-status-dot", "ok"]);
                self.git_label.set_text(&text);
                self.git_label.set_tooltip_text(Some(""));
                let snd = sender.clone();
                // g is closure-baked from this call — the gen IS the token
                glib::timeout_add_local(Duration::from_millis(2000), move || {
                    snd.input(GitPanelMsg::PulseFired(g));
                    glib::ControlFlow::Break
                });
            }
            GitStatusKind::Err => {
                self.dot.set_css_classes(&["alpaca-status-dot", "err"]);
                self.git_label.set_text(text.lines().next().unwrap_or("Git error"));
                let tip: Option<String> = if text.is_empty() { None } else { Some(text) };
                self.git_label.set_tooltip_text(tip.as_deref());
            }
            GitStatusKind::Busy => unreachable!(), // handled above
        }
    }

    /// Sync path (set_root, HEAD monitor, pulse, focus hook): one git call,
    /// paints via paint_status and feeds the diff-tab hook + open changes view
    /// (SYNC git on the UI thread — python parity).
    fn refresh_status(&mut self, sender: &ComponentSender<Self>) {
        let root_str = self.root.as_ref().map(|p| p.to_string_lossy().into_owned());
        let branch = root_str.as_deref().map(gitstatus::branch_of).flatten();
        let raw = match branch {
            Some(_) => gitstatus::changes(root_str.as_ref().unwrap()),
            None => None,
        };
        self.paint_status(branch.as_deref(), raw.as_ref().map(|r| r.len()));
        if let Some(raw) = &raw {
            let _ = sender.output(GitPanelOutput::GitChanged(raw.clone()));
            if self.mode == "changes" {
                // branch Some ⇒ root Some
                self.changes.emit(GitViewMsg::Apply {
                    raw: Some(raw.clone()),
                    ahead: gitstatus::ahead(root_str.as_ref().unwrap()),
                    keep: true,
                });
            }
        }
    }

    /// Statusbar rendering only — git data comes in from callers (the sync
    /// path above or the live probe's worker thread). The commit/push flight
    /// owns the row while busy; `git status` runs OFF-THREAD in both paths.
    fn paint_status(&mut self, branch: Option<&str>, dirty: Option<usize>) {
        if self.git_busy {
            return; // commit/push flight owns the row (pulse re-syncs)
        }
        let is_git = branch.is_some();
        self.branch.emit(BranchMenuMsg::Update(branch.map(str::to_string)));
        self.dot.set_visible(is_git);
        self.spin.set_visible(is_git);
        self.git_label.set_visible(is_git);
        self.spin.set_visible(false); // idle state: the dot, never the spinner
        self.ch_btn.set_visible(is_git);
        if !is_git && self.mode == "changes" {
            self.set_mode("tree"); // repo vanished (HEAD deleted) while reading it
        }
        if !is_git {
            return;
        }
        self.git_label.set_tooltip_text(Some("")); // a past err's tooltip must not outlive the row's re-sync
        // count = porcelain rows (untracked listed per-file, -z -uall) — same
        // number the changes view shows; one git call feeds both (python 305-307)
        match dirty {
            None => {
                self.git_label.set_text("");
                self.dot.set_visible(false);
            }
            Some(0) => {
                self.git_label.set_text("No changes");
                self.dot.set_css_classes(&["alpaca-status-dot", "ok"]);
                self.dot.set_visible(true);
            }
            Some(dirty) => {
                self.git_label.set_text(&format!("{dirty} changed"));
                self.dot.set_css_classes(&["alpaca-status-dot", "warn"]);
                self.dot.set_visible(true);
            }
        }
    }

    /// python _set_mode (257-282): swap the card between the file tree and the
    /// changes view. Entering CHANGES re-syncs the list (§2 refresh trigger:
    /// view becomes active); the selected tab's css class holds it bright,
    /// hover brightens the idle one.
    fn set_mode(&mut self, mode: &'static str) {
        if mode == self.mode {
            return;
        }
        self.mode = mode;
        self.stack.set_visible_child_name(mode);
        let (own, other) = if mode == "tree" {
            (&self.ws_btn, &self.ch_btn)
        } else {
            (&self.ch_btn, &self.ws_btn)
        };
        own.set_css_classes(&["alpaca-tabbtn", "alpaca-on"]);
        other.set_css_classes(&["alpaca-tabbtn"]);
        self.entry.set_placeholder_text(if mode == "tree" {
            Some("Search files…")
        } else {
            Some("Filter changes…")
        });
        if mode == "tree" {
            // the shared entry text survives the tab switch (and its view may
            // be displaced by the last search): a needle re-filters the tree, an
            // empty box restores the displaced sheet instead of leaving the
            // last flat results stuck with no way back (python comments
            // verbatim; the tree's Search("") = its populate+restore, one input)
            let text = self.entry.text().to_string();
            self.ft.emit(FileTreeMsg::Search(text));
        }
        if mode == "changes" {
            self.changes.emit(GitViewMsg::Refresh { keep: true }); // fresh data (re-applies the view's own needle)
            // entry text outranks a stale needle (tree-mode search wrote it)
            let text = self.entry.text().to_string();
            self.changes.emit(GitViewMsg::Filter(text));
        }
    }
}