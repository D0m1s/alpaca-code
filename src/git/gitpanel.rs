use std::path::PathBuf;
use std::time::Duration;

use relm4::component::ComponentController;
use relm4::gtk::{self, gio, glib, pango, prelude::*};
use relm4::prelude::*;

use crate::fs::filetree::{FileTree, FileTreeMsg, FileTreeOutput};
use crate::git::branchmenu::{BranchMenu, BranchMenuMsg, BranchMenuOutput};
use crate::git::gitstatus::{self, GitStatusKind, Rows};
use crate::git::gitview::{ChangesView, GitViewMsg, GitViewOutput};

#[derive(Debug)]
pub enum ProbeChild {
    Changes(GitViewMsg),
    Tree(FileTreeMsg),
}

#[derive(Debug)]
pub enum GitPanelMsg {
    SetRoot(PathBuf),
    ModeTree,
    ModeChanges,
    EntryChanged(String),
    FocusSearch,
    ChildStatus(GitStatusKind, String),
    ChildBusy(bool),
    Count(usize),
    ProbeTick,
    PulseFired(u64),
    RefreshGit,
    StartCommit {
        root: PathBuf,
        paths: Vec<String>,
        msg: String,
    },
    ProbeChild(ProbeChild),
    ProbeEntry(String),
    TreeMin,
    SetTreeState {
        expanded: Vec<String>,
        closed: Vec<String>,
    },
    RestoreTree(Vec<String>),
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
    GitChanged(Rows),
}

#[derive(Debug)]
pub enum GitPanelCommand {
    ProbeLanded {
        root: String,
        branch: Option<String>,
        raw: Option<Rows>,
        ahead: usize,
    },
    StatusLanded {
        root: String,
        branch: Option<String>,
        raw: Option<Rows>,
        ahead: usize,
    },
}

pub struct GitPanel {
    root: Option<PathBuf>,
    mode: &'static str,
    git_busy: bool,
    probe_busy: bool,
    changes_busy: bool,
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
            set_css_classes: &["alpaca-card"],
            set_overflow: gtk::Overflow::Hidden,
        }
    }

    fn init(_: (), root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        let snd_ft = sender.clone();
        let ft = FileTree::builder()
            .launch(())
            .connect_receiver(move |_, out| match out {
                FileTreeOutput::OpenFile(p) => {
                    let _ = snd_ft.output(GitPanelOutput::OpenFile(p));
                }
                FileTreeOutput::Count(n) => {
                    let _ = snd_ft.input_sender().send(GitPanelMsg::Count(n));
                }
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
                GitViewOutput::Status(kind, text) => {
                    let _ = snd_ch
                        .input_sender()
                        .send(GitPanelMsg::ChildStatus(kind, text));
                }
                GitViewOutput::Busy(b) => {
                    let _ = snd_ch.input_sender().send(GitPanelMsg::ChildBusy(b));
                }
            });
        let snd_br = sender.clone();
        let branch = BranchMenu::builder()
            .launch(())
            .connect_receiver(move |_, out| match out {
                BranchMenuOutput::Status(kind, text) => {
                    let _ = snd_br
                        .input_sender()
                        .send(GitPanelMsg::ChildStatus(kind, text));
                }
            });

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
        entry.set_size_request(-1, 30);
        let snd_eg = sender.clone();
        entry.connect_search_changed(move |e| {
            snd_eg.input(GitPanelMsg::EntryChanged(e.text().to_string()));
        });

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
            .css_classes(["alpaca-tabbtn", "alpaca-on"])
            .build();
        let ch_btn = gtk::Button::builder()
            .label("CHANGES")
            .has_frame(false)
            .css_classes(["alpaca-tabbtn"])
            .build();
        ch_btn.set_visible(false);
        let snd_ws = sender.clone();
        ws_btn.connect_clicked(move |_| snd_ws.input(GitPanelMsg::ModeTree));
        let snd_cm = sender.clone();
        ch_btn.connect_clicked(move |_| snd_cm.input(GitPanelMsg::ModeChanges));
        btns.append(&ws_btn);
        btns.append(&ch_btn);

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
        bar.append(branch.widget());
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
        let spacer = gtk::Box::builder().hexpand(true).build();
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

        let snd_tick = sender.clone();
        glib::timeout_add_local(Duration::from_millis(2000), move || {
            if snd_tick
                .input_sender()
                .send(GitPanelMsg::ProbeTick)
                .is_err()
            {
                return glib::ControlFlow::Break;
            }
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
            GitPanelMsg::FocusSearch => {
                self.entry.grab_focus();
            }
            GitPanelMsg::ChildStatus(kind, text) => self.show_git_status(kind, text, &sender),
            GitPanelMsg::ChildBusy(b) => self.changes_busy = b,
            GitPanelMsg::Count(n) => self.count_label.set_text(&format!("{n} files")),
            GitPanelMsg::ProbeTick => self.probe_tick(&sender),
            GitPanelMsg::PulseFired(g) => {
                if g == self.pulse_gen {
                    self.refresh_status(&sender);
                }
            }
            GitPanelMsg::RefreshGit => self.refresh_status(&sender),
            GitPanelMsg::StartCommit { root, paths, msg } => {
                self.changes
                    .emit(GitViewMsg::StartCommit { root, paths, msg });
            }
            GitPanelMsg::ProbeChild(pc) => match pc {
                ProbeChild::Changes(m) => self.changes.emit(m),
                ProbeChild::Tree(m) => self.ft.emit(m),
            },
            GitPanelMsg::SetTreeState { expanded, closed } => {
                self.changes.emit(GitViewMsg::SetClosed(closed));
                let snd = sender.clone();
                glib::idle_add_local_once(move || {
                    if snd
                        .input_sender()
                        .send(GitPanelMsg::RestoreTree(expanded))
                        .is_err()
                    {}
                });
            }
            GitPanelMsg::RestoreTree(expanded) => {
                if let Some(root) = self.root.clone() {
                    self.ft.emit(FileTreeMsg::RestoreTree(root, expanded, 0.0));
                }
            }
            GitPanelMsg::ProbeEntry(t) => self.entry.set_text(&t),
            GitPanelMsg::TreeMin => {
                let min = self.ft.widget().measure(gtk::Orientation::Horizontal, -1).0;
                let ok = min <= 170;
                println!(
                    "PROBE assert: s3-min-tree: {}",
                    if ok { "PASS" } else { "FAIL" }
                );
            }
        }
    }

    fn update_cmd(
        &mut self,
        cmd: GitPanelCommand,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match cmd {
            GitPanelCommand::ProbeLanded {
                root,
                branch,
                raw,
                ahead,
            } => {
                self.probe_busy = false;
                let cur = self.root.as_ref().map(|p| p.to_string_lossy().into_owned());
                if cur.as_deref() != Some(root.as_str()) || self.git_busy || self.changes_busy {
                    return;
                }
                self.paint_status(branch.as_deref(), raw.as_ref().map(|r| r.len()));
                if let Some(raw) = raw {
                    let _ = sender.output(GitPanelOutput::GitChanged(raw.clone()));
                    if self.mode == "changes" {
                        self.changes.emit(GitViewMsg::Apply {
                            raw: Some(raw),
                            ahead,
                            keep: true,
                        });
                    }
                }
            }
            GitPanelCommand::StatusLanded {
                root,
                branch,
                raw,
                ahead,
            } => {
                let cur = self.root.as_ref().map(|p| p.to_string_lossy().into_owned());
                if cur.as_deref() != Some(root.as_str()) {
                    return;
                }
                self.paint_status(branch.as_deref(), raw.as_ref().map(|r| r.len()));
                if let Some(raw) = raw {
                    let _ = sender.output(GitPanelOutput::GitChanged(raw.clone()));
                    if self.mode == "changes" {
                        self.changes.emit(GitViewMsg::Apply {
                            raw: Some(raw),
                            ahead,
                            keep: true,
                        });
                    }
                }
            }
        }
    }
}

impl GitPanel {
    pub fn tree_state(&self) -> (Vec<String>, Vec<String>) {
        (
            self.ft.model().open_dirs(),
            self.changes.model().closed_dirs(),
        )
    }

    fn set_root(&mut self, root: PathBuf, sender: &ComponentSender<Self>) {
        if let Some(m) = self.head_mon.take() {
            m.cancel();
        }
        self.root = Some(root.clone());
        self.entry.set_text("");
        self.ft.emit(FileTreeMsg::SetRoot(root.clone()));
        self.set_mode("tree");
        self.changes.emit(GitViewMsg::SetRoot(root.clone()));
        self.branch.emit(BranchMenuMsg::SetRoot(Some(root.clone())));
        let head = root.join(".git").join("HEAD");
        if head.is_file()
            && let Ok(mon) = gio::File::for_path(&head)
                .monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
        {
            let snd = sender.clone();
            mon.connect_changed(move |_, _, _, _| snd.input(GitPanelMsg::RefreshGit));
            self.head_mon = Some(mon);
        }
        self.refresh_status(sender);
    }

    fn entry_changed(&mut self, text: &str) {
        if self.mode == "tree" {
            self.ft.emit(FileTreeMsg::Search(text.to_string()));
        } else {
            self.changes.emit(GitViewMsg::Filter(text.to_string()));
        }
    }

    fn probe_tick(&mut self, sender: &ComponentSender<Self>) {
        if self.probe_busy || self.git_busy || self.changes_busy {
            return;
        }
        let Some(root) = self.root.as_ref().map(|p| p.to_string_lossy().into_owned()) else {
            return;
        };
        self.probe_busy = true;
        sender.spawn_command(move |out| {
            if std::env::var("ALPACA_PROBE_S2").is_ok() {
                println!(
                    "PROBE probe-flight: root={root} ts={}",
                    crate::ui::app::epoch_ms()
                );
            }
            let raw = gitstatus::changes(&root);
            let ahead = if raw.is_some() {
                gitstatus::ahead(&root)
            } else {
                0
            };
            let branch = gitstatus::branch_of(&root);
            out.emit(GitPanelCommand::ProbeLanded {
                root,
                branch,
                raw,
                ahead,
            });
        });
    }

    fn show_git_status(
        &mut self,
        kind: GitStatusKind,
        text: String,
        sender: &ComponentSender<Self>,
    ) {
        self.pulse_gen += 1;
        let g = self.pulse_gen;
        self.git_busy = kind == GitStatusKind::Busy;
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
                glib::timeout_add_local(Duration::from_millis(2000), move || {
                    if snd.input_sender().send(GitPanelMsg::PulseFired(g)).is_err() {
                        return glib::ControlFlow::Break;
                    }
                    glib::ControlFlow::Break
                });
            }
            GitStatusKind::Err => {
                self.dot.set_css_classes(&["alpaca-status-dot", "err"]);
                self.git_label
                    .set_text(text.lines().next().unwrap_or("Git error"));
                let tip: Option<String> = if text.is_empty() { None } else { Some(text) };
                self.git_label.set_tooltip_text(tip.as_deref());
            }
            GitStatusKind::Busy => unreachable!(),
        }
    }

    fn refresh_status(&mut self, sender: &ComponentSender<Self>) {
        let Some(root) = self.root.as_ref().map(|p| p.to_string_lossy().into_owned()) else {
            return;
        };
        sender.spawn_command(move |out| {
            let raw = gitstatus::changes(&root);
            let ahead = if raw.is_some() {
                gitstatus::ahead(&root)
            } else {
                0
            };
            let branch = gitstatus::branch_of(&root);
            out.emit(GitPanelCommand::StatusLanded {
                root,
                branch,
                raw,
                ahead,
            });
        });
    }

    fn paint_status(&mut self, branch: Option<&str>, dirty: Option<usize>) {
        if self.git_busy {
            return;
        }
        let is_git = branch.is_some();
        self.branch
            .emit(BranchMenuMsg::Update(branch.map(str::to_string)));
        self.dot.set_visible(is_git);
        self.git_label.set_visible(is_git);
        self.spin.set_visible(false);
        self.ch_btn.set_visible(is_git);
        if !is_git && self.mode == "changes" {
            self.set_mode("tree");
        }
        if !is_git {
            return;
        }
        self.git_label.set_tooltip_text(Some(""));
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
            let text = self.entry.text().to_string();
            self.ft.emit(FileTreeMsg::Search(text));
        }
        if mode == "changes" {
            self.changes.emit(GitViewMsg::Refresh { keep: true });
            let text = self.entry.text().to_string();
            self.changes.emit(GitViewMsg::Filter(text));
        }
    }
}
