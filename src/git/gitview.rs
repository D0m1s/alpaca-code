#![allow(deprecated)]

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::PathBuf;

use relm4::gtk::{self, gdk_pixbuf, glib, pango, prelude::*};
use relm4::prelude::*;

use crate::fs::filetree::icon_of;
use crate::fs::treehover::{HoverTree, toggle_hit};
use crate::git::gitstatus::{self, FlightOut, GitStatusKind, Rows};
use crate::render::badges;

pub fn strip_icons(name: &str, is_dir: bool) -> (Option<gdk_pixbuf::Pixbuf>, Option<&'static str>) {
    let art = if is_dir {
        badges::folder_pixbuf()
    } else {
        badges::pixbuf_for(name, false)
    };
    let fallback = if art.is_some() || is_dir {
        None
    } else {
        Some(icon_of(name, false))
    };
    (art, fallback)
}

pub fn letter_color(letter: &str) -> &'static str {
    match letter {
        "M" => "#f2c94c",
        "A" | "U" => "#22c55e",
        "D" => "#ef4444",
        "R" | "T" => "#2f80ed",
        "C" => "#ef4444",
        _ => "#f2c94c",
    }
}

pub fn filtered(rows: &Rows, needle: &str) -> Rows {
    let n = needle.trim().to_ascii_lowercase();
    if n.is_empty() {
        return rows.clone();
    }
    rows.iter()
        .filter(|(rel, _)| {
            let base = rel.rsplit('/').next().unwrap_or(rel).to_lowercase();
            base.contains(&n) || rel.to_lowercase().contains(&n)
        })
        .cloned()
        .collect()
}

pub fn toggled_checked(
    kind: &str,
    rel: &str,
    checked: &BTreeSet<String>,
    visible: &BTreeSet<String>,
) -> Option<BTreeSet<String>> {
    let mut out = checked.clone();
    match kind {
        "f" => {
            if !out.remove(rel) {
                out.insert(rel.to_string());
            }
        }
        "d" | "s" => {
            let prefix = match kind {
                "d" => format!("{rel}/"),
                _ => String::new(),
            };
            let files: Vec<&String> = visible.iter().filter(|r| r.starts_with(&prefix)).collect();
            let all_in = !files.is_empty() && files.iter().all(|f| out.contains(*f));
            if all_in {
                for f in files {
                    out.remove(f);
                }
            } else {
                for f in files {
                    out.insert(f.clone());
                }
            }
        }
        _ => return None,
    }
    Some(out)
}

const COL_NAME: i32 = 0;
const COL_REL: i32 = 1;
const COL_KIND: i32 = 2;
const COL_LETTER: i32 = 3;
const COL_CHEV: i32 = 4;
const COL_BADGE: i32 = 5;
const COL_CHIP: i32 = 6;
const COL_CHECKED: i32 = 7;
const COL_INCONSIST: i32 = 8;
const COL_BADGE_VIS: i32 = 9;
const COL_CHIP_VIS: i32 = 10;
const COL_TOG_VIS: i32 = 11;
const COL_ICON: i32 = 12;
const COL_ICON_VIS: i32 = 13;

#[derive(Debug)]
pub enum GitViewMsg {
    SetRoot(PathBuf),
    Refresh {
        keep: bool,
    },
    Apply {
        raw: Option<Rows>,
        ahead: usize,
        keep: bool,
    },
    Filter(String),
    RowToggled(gtk::TreePath),
    Activated(gtk::TreePath),
    RowExpanded(gtk::TreePath),
    RowCollapsed(gtk::TreePath),
    CommitClicked,
    StartCommit {
        root: PathBuf,
        paths: Vec<String>,
        msg: String,
    },
    SetClosed(Vec<String>),
    ProbeMsg(String),
}

#[derive(Debug, Clone)]
pub enum GitViewOutput {
    Open {
        rel: String,
        letter: String,
    },
    Commit {
        root: PathBuf,
        paths: Vec<String>,
        msg: String,
    },
    Status(GitStatusKind, String),
    Busy(bool),
}

#[derive(Debug)]
pub enum GitViewCommand {
    Phase,
    Landed(FlightOut),
    Refreshed {
        root: PathBuf,
        seq: u64,
        raw: Option<Rows>,
        ahead: usize,
        keep: bool,
    },
}

pub struct ChangesView {
    root: Option<PathBuf>,
    checked: BTreeSet<String>,
    visible: BTreeSet<String>,
    needle: String,
    closed: HashSet<String>,
    rows: Rows,
    shown: Rows,
    ahead: usize,
    refresh_seq: u64,
    busy: bool,
    flight: bool,
    tog_w: Cell<Option<i32>>,
    store: gtk::TreeStore,
    view: HoverTree,
    col: gtk::TreeViewColumn,
    tog: gtk::CellRendererToggle,
    msg: gtk::Entry,
    btn: gtk::Button,
}

#[relm4::component(pub)]
impl Component for ChangesView {
    type CommandOutput = GitViewCommand;
    type Input = GitViewMsg;
    type Output = GitViewOutput;
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
        let store = gtk::TreeStore::new(&[
            glib::Type::STRING,
            glib::Type::STRING,
            glib::Type::STRING,
            glib::Type::STRING,
            gdk_pixbuf::Pixbuf::static_type(),
            gdk_pixbuf::Pixbuf::static_type(),
            gdk_pixbuf::Pixbuf::static_type(),
            glib::Type::BOOL,
            glib::Type::BOOL,
            glib::Type::BOOL,
            glib::Type::BOOL,
            glib::Type::BOOL,
            glib::Type::STRING,
            glib::Type::BOOL,
        ]);

        let view = HoverTree::default();
        view.set_model(Some(&store));
        view.set_headers_visible(false);
        view.set_activate_on_single_click(true);
        view.set_show_expanders(false);
        view.set_level_indentation(16);
        view.set_tooltip_column(1);

        let tree_page = gtk::ScrolledWindow::builder()
            .child(&view)
            .vexpand(true)
            .build();
        root.append(&tree_page);

        let chev = gtk::CellRendererPixbuf::new();
        chev.set_property("xpad", 2u32);
        chev.set_property("ypad", 2u32);
        chev.set_property("yalign", 2.0f32 / 3.0f32);
        let tog = gtk::CellRendererToggle::new();
        tog.set_activatable(false);
        let chip = gtk::CellRendererPixbuf::new();
        chip.set_property("xpad", 2u32);
        chip.set_property("ypad", 2u32);
        chip.set_property("yalign", 1.0f32);
        let badge = gtk::CellRendererPixbuf::new();
        badge.set_property("xpad", 2u32);
        badge.set_property("ypad", 2u32);
        badge.set_property("yalign", 1.0f32);
        let name = gtk::CellRendererText::new();
        name.set_property("xpad", 2u32);
        name.set_property("ypad", 2u32);
        name.set_property("ellipsize", pango::EllipsizeMode::Middle);
        let col = gtk::TreeViewColumn::new();
        col.pack_start(&chev, false);
        col.add_attribute(&chev, "pixbuf", COL_CHEV);
        col.pack_start(&tog, false);
        col.add_attribute(&tog, "active", COL_CHECKED);
        col.add_attribute(&tog, "inconsistent", COL_INCONSIST);
        col.add_attribute(&tog, "visible", COL_TOG_VIS);
        col.pack_start(&chip, false);
        col.add_attribute(&chip, "pixbuf", COL_CHIP);
        col.add_attribute(&chip, "visible", COL_CHIP_VIS);
        col.pack_start(&badge, false);
        col.add_attribute(&badge, "pixbuf", COL_BADGE);
        col.add_attribute(&badge, "visible", COL_BADGE_VIS);
        let icon = gtk::CellRendererPixbuf::new();
        icon.set_property("xpad", 2u32);
        icon.set_property("ypad", 2u32);
        icon.set_property("yalign", 1.0f32);
        col.pack_start(&icon, false);
        col.add_attribute(&icon, "icon-name", COL_ICON);
        col.add_attribute(&icon, "visible", COL_ICON_VIS);
        col.pack_start(&name, true);
        col.add_attribute(&name, "text", COL_NAME);
        col.set_expand(true);
        view.append_column(&col);
        view.set_css_classes(&["alpaca-tree"]);

        let bar = gtk::Box::new(gtk::Orientation::Vertical, 6);
        bar.set_margin_start(12);
        bar.set_margin_end(12);
        bar.set_margin_top(6);
        bar.set_margin_bottom(6);
        bar.set_css_classes(&["alpaca-commitbar"]);
        let msg = gtk::Entry::builder()
            .placeholder_text("Commit message…")
            .hexpand(true)
            .css_classes(["alpaca-msg"])
            .build();
        msg.set_size_request(-1, 30);
        let btn = gtk::Button::builder()
            .label("Commit and Push")
            .hexpand(true)
            .css_classes(["alpaca-barbtn"])
            .build();
        bar.append(&msg);
        bar.append(&btn);
        root.append(&bar);

        let snd = sender.clone();
        view.connect_row_activated(move |_, path, _| {
            snd.input(GitViewMsg::Activated(path.clone()));
        });
        let snd = sender.clone();
        view.connect_row_expanded(move |_, _, path| {
            snd.input(GitViewMsg::RowExpanded(path.clone()));
        });
        let snd = sender.clone();
        view.connect_row_collapsed(move |_, _, path| {
            snd.input(GitViewMsg::RowCollapsed(path.clone()));
        });
        let v2 = view.clone();
        store.connect_row_inserted(move |_, _, _| v2.refresh_hover());
        let v3 = view.clone();
        store.connect_row_deleted(move |_, _| v3.refresh_hover());
        let snd = sender.clone();
        msg.connect_activate(move |_| snd.input(GitViewMsg::CommitClicked));
        let snd = sender.clone();
        btn.connect_clicked(move |_| snd.input(GitViewMsg::CommitClicked));

        let model = ChangesView {
            root: None,
            checked: BTreeSet::new(),
            visible: BTreeSet::new(),
            needle: String::new(),
            closed: HashSet::new(),
            rows: Vec::new(),
            shown: Vec::new(),
            ahead: 0,
            refresh_seq: 0,
            busy: false,
            flight: false,
            tog_w: Cell::new(None),
            store: store.clone(),
            view: view.clone(),
            col: col.clone(),
            tog: tog.clone(),
            msg: msg.clone(),
            btn: btn.clone(),
        };
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: GitViewMsg, sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            GitViewMsg::SetRoot(root) => {
                self.root = Some(root);
                self.closed.clear();
                self.refresh(false, &sender);
            }
            GitViewMsg::Refresh { keep } => self.refresh(keep, &sender),
            GitViewMsg::Apply { raw, ahead, keep } => self.apply(raw, ahead, keep),
            GitViewMsg::Filter(needle) => {
                let n = needle.trim().to_lowercase();
                if n == self.needle.trim().to_lowercase() {
                    return;
                }
                self.needle = needle;
                let shown = filtered(&self.rows, &self.needle);
                self.fill(&shown);
            }
            GitViewMsg::RowToggled(tpath) => self.row_toggled(&tpath),
            GitViewMsg::Activated(tpath) => self.activated(&tpath, &sender),
            GitViewMsg::RowExpanded(tpath) => self.expand_toggle(&tpath, true),
            GitViewMsg::RowCollapsed(tpath) => self.expand_toggle(&tpath, false),
            GitViewMsg::CommitClicked => self.commit_clicked(&sender),
            GitViewMsg::ProbeMsg(t) => self.msg.set_text(&t),
            GitViewMsg::SetClosed(v) => self.closed = v.into_iter().collect(),
            GitViewMsg::StartCommit { root, paths, msg } => {
                self.start_commit(root, paths, msg, &sender)
            }
        }
    }

    fn update_cmd(
        &mut self,
        message: GitViewCommand,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match message {
            GitViewCommand::Phase => {
                let _ = sender.output(GitViewOutput::Status(
                    GitStatusKind::Busy,
                    "Pushing…".into(),
                ));
            }
            GitViewCommand::Landed(out) => {
                self.busy = false;
                let _ = sender.output(GitViewOutput::Busy(false));
                self.flight = false;
                self.buttons();
                if out.cok {
                    self.msg.set_text("");
                    self.refresh(true, &sender);
                }
                if !out.cok {
                    let _ = sender.output(GitViewOutput::Status(GitStatusKind::Err, out.ctext));
                } else if !out.pok {
                    let _ = sender.output(GitViewOutput::Status(GitStatusKind::Err, out.ptext));
                } else {
                    let _ =
                        sender.output(GitViewOutput::Status(GitStatusKind::Ok, "Pushed ✓".into()));
                }
            }
            GitViewCommand::Refreshed {
                root,
                seq,
                raw,
                ahead,
                keep,
            } => {
                if self.root.as_ref() != Some(&root) || seq != self.refresh_seq {
                    return;
                }
                self.apply(raw, ahead, keep);
            }
        }
    }
}

impl ChangesView {
    pub fn closed_dirs(&self) -> Vec<String> {
        self.closed.iter().cloned().collect()
    }

    fn refresh(&mut self, keep: bool, sender: &ComponentSender<Self>) {
        let Some(root) = self.root.clone() else {
            self.apply(None, 0, keep);
            return;
        };
        self.refresh_seq += 1;
        let seq = self.refresh_seq;
        sender.spawn_command(move |out| {
            let root_str = root.to_string_lossy().into_owned();
            let raw = gitstatus::changes(&root_str);
            let ahead = if raw.is_some() {
                gitstatus::ahead(&root_str)
            } else {
                0
            };
            out.emit(GitViewCommand::Refreshed {
                root,
                seq,
                raw,
                ahead,
                keep,
            });
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_row(
        &self,
        parent: Option<&gtk::TreeIter>,
        name: &str,
        rel: &str,
        kind: &str,
        letter: &str,
        chev: &gdk_pixbuf::Pixbuf,
        badge: &gdk_pixbuf::Pixbuf,
        chip: &gdk_pixbuf::Pixbuf,
        checked: bool,
        inconsist: bool,
        badge_vis: bool,
        chip_vis: bool,
        tog_vis: bool,
        icon_name: &str,
        icon_vis: bool,
    ) -> gtk::TreeIter {
        let vals: Vec<(u32, &dyn glib::value::ToValue)> = vec![
            (0, &name),
            (1, &rel),
            (2, &kind),
            (3, &letter),
            (4, chev),
            (5, badge),
            (6, chip),
            (7, &checked),
            (8, &inconsist),
            (9, &badge_vis),
            (10, &chip_vis),
            (11, &tog_vis),
            (12, &icon_name),
            (13, &icon_vis),
        ];
        self.store.insert_with_values(parent, None, &vals)
    }

    fn apply(&mut self, raw: Option<Rows>, ahead: usize, keep: bool) {
        let Some(raw) = raw else {
            return;
        };
        self.ahead = ahead;
        if keep && raw == self.rows {
            return;
        }
        self.rows = raw;
        let fset: BTreeSet<String> = gitstatus::group_tree(&self.rows)
            .iter()
            .filter(|t| t.kind == "f")
            .map(|t| t.rel.clone())
            .collect();
        self.checked = if keep {
            self.checked.intersection(&fset).cloned().collect()
        } else {
            fset
        };
        let shown = filtered(&self.rows, &self.needle);
        self.fill(&shown);
    }

    fn fill(&mut self, shown: &Rows) {
        self.store.clear();
        self.shown = shown.clone();
        self.visible = gitstatus::group_tree(shown)
            .iter()
            .filter(|t| t.kind == "f")
            .map(|t| t.rel.clone())
            .collect();
        let blank = badges::blank_pixbuf();
        if shown.is_empty() {
            self.insert_row(
                None,
                "No changes",
                "",
                "e",
                "",
                &blank,
                &blank,
                &blank,
                false,
                false,
                false,
                false,
                false,
                "",
                false,
            );
            self.sync();
            return;
        }
        self.insert_row(
            None,
            "Select all",
            "",
            "s",
            "",
            &blank,
            &blank,
            &blank,
            false,
            false,
            false,
            false,
            true,
            "",
            false,
        );
        let mut iters: BTreeMap<String, gtk::TreeIter> = BTreeMap::new();
        for t in gitstatus::group_tree(shown) {
            let is_dir = t.kind == "d";
            let (art, fallback) = strip_icons(&t.name, is_dir);
            let badge = art.clone().unwrap_or_else(badges::blank_pixbuf);
            let lpix = if !t.letter.is_empty() {
                badges::letter_pixbuf(&t.letter, letter_color(&t.letter))
                    .unwrap_or_else(badges::blank_pixbuf)
            } else {
                badges::blank_pixbuf()
            };
            let chev = if is_dir {
                badges::chevron_pixbuf(false).unwrap_or_else(badges::blank_pixbuf)
            } else {
                badges::blank_pixbuf()
            };
            let parent = t
                .rel
                .rsplit_once('/')
                .and_then(|(dir, _)| iters.get(dir).cloned());
            let badge_vis = art.is_some();
            let chip_vis = !is_dir;
            let tog_vis = true;
            let it = self.insert_row(
                parent.as_ref(),
                &t.name,
                &t.rel,
                t.kind,
                &t.letter,
                &chev,
                &badge,
                &lpix,
                false,
                false,
                badge_vis,
                chip_vis,
                tog_vis,
                fallback.unwrap_or_default(),
                fallback.is_some(),
            );
            if is_dir {
                iters.insert(t.rel.clone(), it);
            }
        }
        self.sync();
        for (rel, it) in &iters {
            if !self.closed.contains(rel) {
                self.view.expand_row(&self.store.path(it), false);
            }
        }
    }

    fn sync(&mut self) {
        let st = if self.shown.is_empty() {
            BTreeMap::new()
        } else {
            let lensed: BTreeSet<String> =
                self.visible.intersection(&self.checked).cloned().collect();
            gitstatus::checked_dir_state(&gitstatus::group_tree(&self.shown), &lensed)
        };
        let mut cur = self.store.iter_first();
        while let Some(i) = cur.take() {
            self.sync_row(&i, &st);
            let mut t = i;
            cur = if self.store.iter_next(&mut t) {
                Some(t)
            } else {
                None
            };
            self.walk_children(&i, &st);
        }
        self.buttons();
    }

    fn walk_children(&self, parent: &gtk::TreeIter, st: &BTreeMap<String, (bool, bool)>) {
        let mut cur = self.store.iter_children(Some(parent));
        while let Some(i) = cur.take() {
            self.sync_row(&i, st);
            let mut t = i;
            cur = if self.store.iter_next(&mut t) {
                Some(t)
            } else {
                None
            };
            self.walk_children(&i, st);
        }
    }

    fn sync_row(&self, it: &gtk::TreeIter, st: &BTreeMap<String, (bool, bool)>) {
        let kind = self
            .store
            .get_value(it, COL_KIND)
            .get::<String>()
            .unwrap_or_default();
        if kind == "f" {
            let rel = self
                .store
                .get_value(it, COL_REL)
                .get::<String>()
                .unwrap_or_default();
            let v = self.checked.contains(&rel);
            self.store.set_value(it, COL_CHECKED as u32, &v.to_value());
        } else if kind == "d" {
            let rel = self
                .store
                .get_value(it, COL_REL)
                .get::<String>()
                .unwrap_or_default();
            let (allc, somec) = *st.get(&rel).unwrap_or(&(false, false));
            self.store
                .set_value(it, COL_CHECKED as u32, &allc.to_value());
            self.store
                .set_value(it, COL_INCONSIST as u32, &(!allc && somec).to_value());
        } else if kind == "s" {
            let allc =
                !self.visible.is_empty() && self.visible.iter().all(|r| self.checked.contains(r));
            let somec = self.checked.iter().any(|r| self.visible.contains(r));
            self.store
                .set_value(it, COL_CHECKED as u32, &allc.to_value());
            self.store
                .set_value(it, COL_INCONSIST as u32, &(!allc && somec).to_value());
        }
    }

    fn buttons(&self) {
        self.btn
            .set_sensitive(!self.busy && (!self.checked.is_empty() || self.ahead > 0));
        self.msg.set_sensitive(!self.busy);
    }

    fn row_toggled(&mut self, tpath: &gtk::TreePath) {
        let Some(it) = self.store.iter(tpath) else {
            return;
        };
        let kind = self
            .store
            .get_value(&it, COL_KIND)
            .get::<String>()
            .unwrap_or_default();
        let rel = self
            .store
            .get_value(&it, COL_REL)
            .get::<String>()
            .unwrap_or_default();
        if let Some(next) = toggled_checked(&kind, &rel, &self.checked, &self.visible) {
            self.checked = next;
            self.sync();
        }
    }

    fn expand_toggle(&mut self, tpath: &gtk::TreePath, expanded: bool) {
        let Some(it) = self.store.iter(tpath) else {
            return;
        };
        let rel = self
            .store
            .get_value(&it, COL_REL)
            .get::<String>()
            .unwrap_or_default();
        if rel.is_empty() {
            return;
        }
        if expanded {
            self.closed.remove(&rel);
        } else {
            self.closed.insert(rel.clone());
        }
        let chev = badges::chevron_pixbuf(expanded).unwrap_or_else(badges::blank_pixbuf);
        self.store.set_value(&it, COL_CHEV as u32, &chev.to_value());
        if expanded {
            let mut cur = self.store.iter_children(Some(&it));
            while let Some(i) = cur.take() {
                let kind = self
                    .store
                    .get_value(&i, COL_KIND)
                    .get::<String>()
                    .unwrap_or_default();
                let child_rel = self
                    .store
                    .get_value(&i, COL_REL)
                    .get::<String>()
                    .unwrap_or_default();
                if kind == "d" && !self.closed.contains(&child_rel) {
                    self.view.expand_row(&self.store.path(&i), false);
                }
                let mut t = i;
                cur = if self.store.iter_next(&mut t) {
                    Some(t)
                } else {
                    None
                };
            }
        }
    }

    fn activated(&mut self, tpath: &gtk::TreePath, sender: &ComponentSender<Self>) {
        let (px, _py) = self.view.press_xy();
        if px >= 0.0 {
            let ca = self.view.cell_area(Some(tpath), Some(&self.col));
            if ca.width() > 0 && toggle_hit(px, ca.x(), self.tog_w()) {
                sender.input(GitViewMsg::RowToggled(tpath.clone()));
                return;
            }
        }
        let Some(it) = self.store.iter(tpath) else {
            return;
        };
        let kind = self
            .store
            .get_value(&it, COL_KIND)
            .get::<String>()
            .unwrap_or_default();
        let rel = self
            .store
            .get_value(&it, COL_REL)
            .get::<String>()
            .unwrap_or_default();
        if kind == "d" && !rel.is_empty() {
            if self.closed.contains(&rel) {
                self.view.expand_row(tpath, false);
            } else {
                self.view.collapse_row(tpath);
            }
        } else if kind == "f" {
            let letter = self
                .store
                .get_value(&it, COL_LETTER)
                .get::<String>()
                .unwrap_or_default();
            let _ = sender.output(GitViewOutput::Open { rel, letter });
        }
    }

    fn tog_w(&self) -> i32 {
        if let Some(w) = self.tog_w.get() {
            return w;
        }
        let (min, natural) = self.tog.preferred_width(&self.view);
        let w = if natural > 0 {
            natural
        } else if min > 0 {
            min
        } else {
            24
        };
        self.tog_w.set(Some(w));
        w
    }

    fn commit_clicked(&mut self, sender: &ComponentSender<Self>) {
        if self.busy || self.root.is_none() {
            return;
        }
        let paths: Vec<String> = self.checked.iter().cloned().collect();
        if paths.is_empty() && self.ahead == 0 {
            let _ = sender.output(GitViewOutput::Status(
                GitStatusKind::Err,
                "No files selected".into(),
            ));
            return;
        }
        let msg = self.msg.text().trim().to_string();
        if !paths.is_empty() && msg.is_empty() {
            let _ = sender.output(GitViewOutput::Status(
                GitStatusKind::Err,
                "Empty commit message".into(),
            ));
            return;
        }
        let root = self.root.clone().unwrap();
        self.busy = true;
        self.buttons();
        let _ = sender.output(GitViewOutput::Status(
            GitStatusKind::Busy,
            if paths.is_empty() {
                "Pushing…"
            } else {
                "Committing…"
            }
            .into(),
        ));
        let _ = sender.output(GitViewOutput::Busy(true));
        let _ = sender.output(GitViewOutput::Commit { root, paths, msg });
    }

    fn start_commit(
        &mut self,
        root: PathBuf,
        paths: Vec<String>,
        msg: String,
        sender: &ComponentSender<Self>,
    ) {
        if self.flight {
            return;
        }
        self.flight = true;
        sender.spawn_command(move |out| {
            let out_f = if paths.is_empty() {
                gitstatus::commit_then_push(&root.to_string_lossy(), &paths, &msg, None)
            } else {
                let outp = out.clone();
                let phase_fn = move |_k: &str| outp.emit(GitViewCommand::Phase);
                gitstatus::commit_then_push(&root.to_string_lossy(), &paths, &msg, Some(&phase_fn))
            };
            out.emit(GitViewCommand::Landed(out_f));
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn rows_of(v: &[(&str, &str)]) -> Rows {
        v.iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn letter_color_table() {
        assert_eq!(letter_color("M"), "#f2c94c");
        assert_eq!(letter_color("A"), "#22c55e");
        assert_eq!(letter_color("D"), "#ef4444");
        assert_eq!(letter_color("R"), "#2f80ed");
        assert_eq!(letter_color("T"), "#2f80ed");
        assert_eq!(letter_color("C"), "#ef4444");
        assert_eq!(letter_color("Q"), "#f2c94c", "unknown letter → default");
    }

    #[test]
    fn filtered_matches_basename_or_rel() {
        let rows = rows_of(&[("z.md", "M"), ("a/b/c.md", "A")]);
        assert_eq!(filtered(&rows, ""), rows, "empty needle → identity");
        assert_eq!(filtered(&rows, "c.md")[0].0, "a/b/c.md", "basename match");
        assert_eq!(
            filtered(&rows, "A/b")[0].0,
            "a/b/c.md",
            "rel match, case-insensitive"
        );
        let both = filtered(&rows, "md");
        assert_eq!(both.len(), 2, "both names carry md");
        assert!(filtered(&rows, "zzz").is_empty());
    }

    #[test]
    fn toggled_checked_laws() {
        let vis = BTreeSet::from([
            "a/c.txt".to_string(),
            "a/d/m.md".to_string(),
            "e.md".to_string(),
        ]);
        let ck = BTreeSet::from(["a/c.txt".to_string(), "a/d/m.md".to_string()]);
        let c = toggled_checked("f", "a/d/m.md", &ck, &vis).unwrap();
        assert!(!c.contains("a/d/m.md") && c.contains("a/c.txt"));
        let ck2 = BTreeSet::from(["a/c.txt".to_string()]);
        let c = toggled_checked("d", "a/d", &ck2, &vis).unwrap();
        assert!(c.contains("a/d/m.md"), "none selected → add all under dir");
        let full = BTreeSet::from([
            "a/c.txt".to_string(),
            "a/d/m.md".to_string(),
            "e.md".to_string(),
        ]);
        let c = toggled_checked("d", "a", &full, &vis).unwrap();
        assert_eq!(
            c,
            BTreeSet::from(["e.md".to_string()]),
            "descendants cleared, rest kept"
        );
        let c = toggled_checked("s", "", &full, &vis).unwrap();
        assert!(c.is_empty());
        let c = toggled_checked("s", "", &BTreeSet::new(), &vis).unwrap();
        assert_eq!(c, vis);
        let c = toggled_checked("d", "zzz", &ck, &vis).unwrap();
        assert_eq!(c, ck);
        assert!(toggled_checked("e", "x", &ck, &vis).is_none());
    }

    #[test]
    fn strip_icons_law() {
        assert_eq!(strip_icons("editor.rs", false).1, None);
        assert!(strip_icons("editor.rs", false).0.is_some());
        assert_eq!(strip_icons("LICENSE", false).1, None);
        assert!(strip_icons("whatever.zzz", false).0.is_some());
        assert_eq!(strip_icons("src", true).1, None);
    }
}
