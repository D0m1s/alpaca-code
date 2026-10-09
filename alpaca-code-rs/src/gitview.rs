//! CHANGES view: git-change tree (group_tree rows) with checkboxes and a
//! commit bar — python gitview.py port. Pure helpers testable displayless;
//! widget-layer is gated by the T8 probe (S1 precedent).
//!
//! The store is REBUILT per refresh (python gitview.py:1-4, the filetree
//! invariant: value-only updates are safe under expanded rows, structural
//! mutation collapses them — surgical patching is off the table here).

// The module's whole API surface is consumed by GitPanel and app wiring
// (proven by the landed callers). NO dead-code allowance survives the T4
// sweep — the deprecation allowance below is LIVE, not transitional: the
// tree-store/iter code uses 81 4.10-deprecated gtk methods (TreeModelExt::iter
// et al., measured — deleting the allow resurrects all 81 warnings).
#![allow(deprecated)]

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::PathBuf;

use relm4::gtk::{self, gdk_pixbuf, glib, pango, prelude::*};
use relm4::prelude::*;

use crate::badges;
use crate::filetree::icon_of;
use crate::gitstatus::{self, FlightOut, GitStatusKind, Rows};
use crate::treehover::{HoverTree, toggle_hit};

/// CHANGES strip icon law — WORKSPACE parity (filetree.rs push_row): chip art
/// when the extension has a badge, else the themed symbolic fallback when it
/// doesn't (`.rs`, no ext, …). Dirs keep folder art; blank only if that art
/// itself failed (embedded asset — never misses).
pub fn strip_icons(name: &str, is_dir: bool) -> (Option<gdk_pixbuf::Pixbuf>, Option<&'static str>) {
    let art = if is_dir {
        badges::folder_pixbuf()
    } else {
        badges::pixbuf_for(name, false)
    };
    let fallback =
        if art.is_some() || is_dir { None } else { Some(icon_of(name, false)) };
    (art, fallback)
}

/// python LETTER_COLOR (gitview.py:17-21) — M amber, A/U green, D red,
/// R blue (+T typechange), C conflict red (I1).
pub fn letter_color(letter: &str) -> &'static str {
    match letter {
        "M" => "#f2c94c",
        "A" | "U" => "#22c55e",
        "D" => "#ef4444",
        "R" | "T" => "#2f80ed",
        "C" => "#ef4444",
        _ => "#f2c94c", // LETTER_DEFAULT
    }
}

/// python _filtered — name/rel substring, case-insensitive; empty needle →
/// the same Vec (identity).
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

/// python _on_toggled's set law, decision-extracted: f flips; d/s use the
/// visible-descendant law (all-under ⇒ clear, else add-all). Dir with no
/// visible descendants → |= ∅ (Some, unchanged). Unknown kind → None.
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

// store column indexes (12 cols — column i = python row list index i)
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
    Refresh { keep: bool },
    Apply { raw: Option<Rows>, ahead: usize, keep: bool },
    Filter(String),
    RowToggled(gtk::TreePath),
    Activated(gtk::TreePath),
    RowExpanded(gtk::TreePath),
    RowCollapsed(gtk::TreePath),
    CommitClicked,
    StartCommit { root: PathBuf, paths: Vec<String>, msg: String },
    /// probe-only: sets the commit-bar entry text (T8's step — the entry is a
    /// user-typed widget; production never constructs this)
    ProbeMsg(String),
}

#[derive(Debug, Clone)]
pub enum GitViewOutput {
    Open { rel: String, letter: String },
    Commit { root: PathBuf, paths: Vec<String>, msg: String },
    Status(GitStatusKind, String),
    Busy(bool),
}

#[derive(Debug)]
pub enum GitViewCommand {
    Phase,
    Landed(FlightOut),
}

pub struct ChangesView {
    root: Option<PathBuf>,
    checked: BTreeSet<String>,
    visible: BTreeSet<String>,
    needle: String,
    closed: HashSet<String>, // user-collapsed dir rels (dirs render OPEN unless closed)
    rows: Rows,
    shown: Rows,
    ahead: usize,
    busy: bool,
    flight: bool,
    tog_w: Cell<Option<i32>>, // toggle slot width, measured lazily once
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
            set_overflow: gtk::Overflow::Hidden, // clip children to rounded corners
        }
    }

    fn init(_: (), root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        // python gitview.py:33-42 — store columns, 12 in python; +2 here (rust
        // only): WORKSPACE's symbolic fallback for artless exts (filetree law)
        let store = gtk::TreeStore::new(&[
            glib::Type::STRING,                // 0 name
            glib::Type::STRING,                // 1 rel
            glib::Type::STRING,                // 2 kind
            glib::Type::STRING,                // 3 letter
            gdk_pixbuf::Pixbuf::static_type(), // 4 chevron
            gdk_pixbuf::Pixbuf::static_type(), // 5 badge
            gdk_pixbuf::Pixbuf::static_type(), // 6 chip
            glib::Type::BOOL,                  // 7 checked
            glib::Type::BOOL,                  // 8 inconsist
            glib::Type::BOOL,                  // 9 badge-vis
            glib::Type::BOOL,                  // 10 chip-vis
            glib::Type::BOOL,                  // 11 toggle-vis
            glib::Type::STRING,                // 12 symbolic icon-name (artless exts)
            glib::Type::BOOL,                  // 13 icon-vis
        ]);

        let view = HoverTree::default();
        view.set_model(Some(&store));
        view.set_headers_visible(false);
        view.set_activate_on_single_click(true);
        view.set_show_expanders(false); // python-exact; our own chevron cell
        view.set_level_indentation(16);
        view.set_tooltip_column(1);

        let tree_page = gtk::ScrolledWindow::builder()
            .child(&view)
            .vexpand(true)
            .build();
        root.append(&tree_page);

        // ONE merged column (gitview.py:59-96): all visual cells pack into one
        // tree column so level indentation shifts the whole strip together;
        // set_expand(true) on the COLUMN — an ellipsized cell's natural is its
        // minimum, slack must ride the column (spec §29)
        let chev = gtk::CellRendererPixbuf::new();
        chev.set_property("xpad", 2u32);
        chev.set_property("ypad", 2u32);
        chev.set_property("yalign", 2.0f32 / 3.0f32); // bottom-pin: same law as filetree
        // checkbox: NOT activatable — clicks route through press geometry
        let tog = gtk::CellRendererToggle::new();
        tog.set_activatable(false);
        // letter chip: chip-vis (=col10) binds `not is_dir` — a letterless FILE
        // row keeps its slot (blank pixbuf); dirs hide it
        let chip = gtk::CellRendererPixbuf::new();
        chip.set_property("xpad", 2u32);
        chip.set_property("ypad", 2u32);
        chip.set_property("yalign", 1.0f32); // bottom-pin at the 22px row
        let badge = gtk::CellRendererPixbuf::new();
        badge.set_property("xpad", 2u32);
        badge.set_property("ypad", 2u32);
        badge.set_property("yalign", 1.0f32);
        let name = gtk::CellRendererText::new();
        name.set_property("xpad", 2u32); // WORKSPACE parity: ink at art+4
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
        // emblem fallback (filetree.rs cells): themed symbolic icon when the
        // chip art is absent — same strip, badge hidden that row
        let icon = gtk::CellRendererPixbuf::new();
        icon.set_property("xpad", 2u32);
        icon.set_property("ypad", 2u32);
        icon.set_property("yalign", 1.0f32); // bottom-pin: same law as the badge cell
        col.pack_start(&icon, false);
        col.add_attribute(&icon, "icon-name", COL_ICON);
        col.add_attribute(&icon, "visible", COL_ICON_VIS);
        col.pack_start(&name, true); // text cell LAST with expand=True — pins the strip left
        col.add_attribute(&name, "text", COL_NAME);
        col.set_expand(true);
        view.append_column(&col);
        view.set_css_classes(&["alpaca-tree"]);

        // commit bar (python 162-186)
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
            .hexpand(true) // full-width pill: same width as the field above
            .css_classes(["alpaca-barbtn"])
            .build();
        bar.append(&msg);
        bar.append(&btn);
        root.append(&bar);

        // signals
        let snd = sender.clone();
        view.connect_row_activated(move |_, path, _| {
            snd.input(GitViewMsg::Activated(path.clone()));
        });
        // the expand-state walker runs on the RowExpanded/RowCollapsed inputs —
        // Activated's dir branch only calls expand/collapse_row and the SIGNAL
        // owns the bookkeeping (python-exact)
        let snd = sender.clone();
        view.connect_row_expanded(move |_, _, path| {
            snd.input(GitViewMsg::RowExpanded(path.clone()));
        });
        let snd = sender.clone();
        view.connect_row_collapsed(move |_, _, path| {
            snd.input(GitViewMsg::RowCollapsed(path.clone()));
        });
        // hover band must follow store mutations (rows inserted/deleted while
        // the pointer stays put)
        let v2 = view.clone();
        store.connect_row_inserted(move |_, _, _| v2.refresh_hover());
        let v3 = view.clone();
        store.connect_row_deleted(move |_, _| v3.refresh_hover());
        let snd = sender.clone();
        msg.connect_activate(move |_| snd.input(GitViewMsg::CommitClicked)); // Enter = commit
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
                self.closed.clear(); // new repo: remembered collapses are stale
                self.refresh(false); // fresh view selects all
                // needle NOT touched here: the panel clears its entry and
                // routes Filter("") on a root switch (brief-documented port).
            }
            GitViewMsg::Refresh { keep } => self.refresh(keep),
            GitViewMsg::Apply { raw, ahead, keep } => self.apply(raw, ahead, keep),
            GitViewMsg::Filter(needle) => {
                // identical-needle guard (the apply guard's twin): mode entry
                // refires Filter with the unchanged entry text — the store
                // already shows that lens. Compare normalized (filtered()
                // lowercases+strips per use).
                let n = needle.trim().to_lowercase();
                if n == self.needle.trim().to_lowercase() {
                    return;
                }
                // stored AS-GIVEN; filtered() strips+lowercases per use —
                // _checked untouched (view-only lens, python comment)
                self.needle = needle;
                let shown = filtered(&self.rows, &self.needle);
                self.fill(&shown);
            }
            GitViewMsg::RowToggled(tpath) => self.row_toggled(&tpath),
            GitViewMsg::Activated(tpath) => self.activated(&tpath, &sender),
            GitViewMsg::RowExpanded(tpath) => self.expand_toggle(&tpath, true),
            GitViewMsg::RowCollapsed(tpath) => self.expand_toggle(&tpath, false),
            GitViewMsg::CommitClicked => self.commit_clicked(&sender),
            GitViewMsg::ProbeMsg(t) => self.msg.set_text(&t), // probe-only
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
                let _ =
                    sender.output(GitViewOutput::Status(GitStatusKind::Busy, "Pushing…".into()));
            }
            GitViewCommand::Landed(out) => {
                self.busy = false;
                // review fix-1: the eager CommitClicked busy(true) MUST be
                // cleared here too — the panel (ChildBusy) can't read this
                // flag, and a latched true gates its 2s probe forever after
                // the first commit (python filetree.py reads _busy live)
                let _ = sender.output(GitViewOutput::Busy(false));
                self.flight = false;
                self.buttons();
                if out.cok {
                    self.msg.set_text(""); // user ruling: the message dies with the commit
                    self.refresh(true); // keep_selection=True: committed paths vanish, rest kept
                }
                if !out.cok {
                    let _ = sender.output(GitViewOutput::Status(GitStatusKind::Err, out.ctext));
                } else if !out.pok {
                    // commit landed; rows refreshed — the red line explains the
                    // ahead count on the next natural refresh
                    let _ = sender.output(GitViewOutput::Status(GitStatusKind::Err, out.ptext));
                } else {
                    // row pulse; filetree re-syncs itself after 2s
                    let _ = sender.output(GitViewOutput::Status(GitStatusKind::Ok, "Pushed ✓".into()));
                }
            }
        }
    }
}

// ---- data (python gitview.py 118-141, 143-152, 154-165) ----------------------
impl ChangesView {
    /// python `refresh` — the git calls run HERE, landing is apply().
    fn refresh(&mut self, keep: bool) {
        let root = self.root.clone();
        let raw = root.as_ref().and_then(|r| gitstatus::changes(&r.to_string_lossy()));
        let ahead = if raw.is_some() {
            root.as_ref().map_or(0, |r| gitstatus::ahead(&r.to_string_lossy()))
        } else {
            0
        };
        self.apply(raw, ahead, keep);
    }

    /// one merged row append; the 12 (col, value) pairs are the python row
    /// list spelled by index.
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

    /// Landing pad for fresh git data — the sync refresh, or the live probe's
    /// worker via Apply. Rebuilds rows, intersects the selection with the
    /// surviving file set, re-applies any active filter. None (no repo /
    /// timeout) is a clean no-op.
    fn apply(&mut self, raw: Option<Rows>, ahead: usize, keep: bool) {
        let Some(raw) = raw else {
            return; // no repo / timeout
        };
        self.ahead = ahead; // cached: no git per toggle
        // identical-porcelain guard: in CHANGES mode the 2s probe re-lands the
        // SAME rows forever (its flight is ~43ms, the fill of a 31k-row repo
        // ~2s — misiuscode's unignored target/ measured it) and the mainloop
        // saturates into a hard hang. Rebuild only when the data moved;
        // keep=false (SetRoot fresh view) still fills so the select-all reset
        // keeps running.
        if keep && raw == self.rows {
            return;
        }
        self.rows = raw;
        // every file row's rel — computed BEFORE fill (sync_row lenses need it)
        let fset: BTreeSet<String> = gitstatus::group_tree(&self.rows)
            .iter()
            .filter(|t| t.kind == "f")
            .map(|t| t.rel.clone())
            .collect();
        self.checked = if keep {
            self.checked.intersection(&fset).cloned().collect()
        } else {
            fset // fresh view selects all (python: refresh(false))
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
                None, "No changes", "", "e", "", &blank, &blank, &blank, false, false, false,
                false, false, "", false,
            ); // cols 7-11,13 ALL false
            self.sync();
            return;
        }
        self.insert_row(
            None, "Select all", "", "s", "", &blank, &blank, &blank, false, false, false, false,
            true, "", false,
        ); // only toggle-vis col11 true
        // BTreeMap: parents sort before any descendant (prefix < child), so the
        // append-parent lookups and the expand pass below see ancestors before
        // children — python gets the same from its insertion-ordered dict.
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
            let badge_vis = art.is_some(); // badge-vis; artless rows show the fallback instead
            let chip_vis = !is_dir; // chip-vis (file rows: a letterless file keeps its slot)
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
        // dirs render open unless the user closed them — expand state lives in
        // `closed`, never read back from the view (filetree lesson)
        for (rel, it) in &iters {
            if !self.closed.contains(rel) {
                self.view.expand_row(&self.store.path(it), false);
            }
        }
    }

    // ---- checked-state sync (value-only writes: safe under expanded rows) ----

    /// dir/masthead state is computed over what the USER SEES (shown ×
    /// visible) with the lensed selection — invisible files never drive a
    /// rendered checkbox (Filter renders a subset; toggles stay visible-only).
    fn sync(&mut self) {
        let st = if self.shown.is_empty() {
            BTreeMap::new()
        } else {
            let lensed: BTreeSet<String> = self
                .visible
                .intersection(&self.checked)
                .cloned()
                .collect();
            gitstatus::checked_dir_state(&gitstatus::group_tree(&self.shown), &lensed)
        };
        let mut cur = self.store.iter_first();
        while let Some(i) = cur.take() {
            self.sync_row(&i, &st);
            // capture next BEFORE recursion (mutation guard, python _sync)
            let mut t = i.clone();
            cur = if self.store.iter_next(&mut t) { Some(t) } else { None };
            self.walk_children(&i, &st);
        }
        self.buttons();
    }

    fn walk_children(&self, parent: &gtk::TreeIter, st: &BTreeMap<String, (bool, bool)>) {
        let mut cur = self.store.iter_children(Some(parent));
        while let Some(i) = cur.take() {
            self.sync_row(&i, st);
            let mut t = i.clone();
            cur = if self.store.iter_next(&mut t) { Some(t) } else { None };
            self.walk_children(&i, st);
        }
    }

    fn sync_row(&self, it: &gtk::TreeIter, st: &BTreeMap<String, (bool, bool)>) {
        let kind = self.store.get_value(it, COL_KIND).get::<String>().unwrap_or_default();
        if kind == "f" {
            let rel = self.store.get_value(it, COL_REL).get::<String>().unwrap_or_default();
            let v = self.checked.contains(&rel);
            self.store.set_value(it, COL_CHECKED as u32, &v.to_value());
        } else if kind == "d" {
            let rel = self.store.get_value(it, COL_REL).get::<String>().unwrap_or_default();
            let (allc, somec) = *st.get(&rel).unwrap_or(&(false, false));
            self.store.set_value(it, COL_CHECKED as u32, &allc.to_value());
            self.store.set_value(it, COL_INCONSIST as u32, &(!allc && somec).to_value());
        } else if kind == "s" {
            let allc = !self.visible.is_empty()
                && self.visible.iter().all(|r| self.checked.contains(r));
            let somec = self.checked.iter().any(|r| self.visible.contains(r));
            self.store.set_value(it, COL_CHECKED as u32, &allc.to_value());
            self.store.set_value(it, COL_INCONSIST as u32, &(!allc && somec).to_value());
        }
        // "e" (No changes) rows get nothing
    }

    /// one button: commit-with-push when files are checked, push-only when the
    /// branch is ahead (nothing checked); ahead from the refresh cache.
    fn buttons(&self) {
        self.btn
            .set_sensitive(!self.busy && (!self.checked.is_empty() || self.ahead > 0));
        self.msg.set_sensitive(!self.busy);
    }

    // ---- toggles ------------------------------------------------------------

    fn row_toggled(&mut self, tpath: &gtk::TreePath) {
        let Some(it) = self.store.iter(tpath) else {
            return;
        };
        let kind = self.store.get_value(&it, COL_KIND).get::<String>().unwrap_or_default();
        let rel = self.store.get_value(&it, COL_REL).get::<String>().unwrap_or_default();
        if let Some(next) = toggled_checked(&kind, &rel, &self.checked, &self.visible) {
            self.checked = next;
            self.sync(); // masthead/dir/file all through the one law
        }
    }

    /// python _on_expand_toggle — wired to RowExpanded/RowCollapsed inputs.
    fn expand_toggle(&mut self, tpath: &gtk::TreePath, expanded: bool) {
        let Some(it) = self.store.iter(tpath) else {
            return;
        };
        let rel = self.store.get_value(&it, COL_REL).get::<String>().unwrap_or_default();
        if rel.is_empty() {
            return;
        }
        if expanded {
            self.closed.remove(&rel); // discard-on-expand (python discard/add pair)
        } else {
            self.closed.insert(rel.clone());
        }
        let chev = badges::chevron_pixbuf(expanded).unwrap_or_else(badges::blank_pixbuf);
        self.store.set_value(&it, COL_CHEV as u32, &chev.to_value());
        if expanded {
            // hidden dirs don't expand (expand_row is a no-op under a collapsed
            // ancestor) — walk one level and catch them up
            let mut cur = self.store.iter_children(Some(&it));
            while let Some(i) = cur.take() {
                let kind = self.store.get_value(&i, COL_KIND).get::<String>().unwrap_or_default();
                let child_rel = self.store.get_value(&i, COL_REL).get::<String>().unwrap_or_default();
                if kind == "d" && !self.closed.contains(&child_rel) {
                    self.view.expand_row(&self.store.path(&i), false);
                }
                let mut t = i.clone();
                cur = if self.store.iter_next(&mut t) { Some(t) } else { None };
            }
        }
    }

    // ---- activation (spec §2: file click opens the diff) ----------------------

    fn activated(&mut self, tpath: &gtk::TreePath, sender: &ComponentSender<Self>) {
        // 1. toggle-hit: press_x inside the toggle's own slot? With one merged
        //    column the checkbox is found by press geometry: [chevron slot end,
        //    + toggle nat) (Review Focus #1)
        let (px, _py) = self.view.press_xy();
        if px >= 0.0 {
            // python gates on `ca is None`; this binding always fills a rect —
            // an unallocated cell comes back width 0 → the same reject
            let ca = self.view.cell_area(Some(tpath), Some(&self.col));
            if ca.width() > 0 && toggle_hit(px, ca.x(), self.tog_w()) {
                sender.input(GitViewMsg::RowToggled(tpath.clone()));
                return;
            }
        }
        let Some(it) = self.store.iter(tpath) else {
            return;
        };
        let kind = self.store.get_value(&it, COL_KIND).get::<String>().unwrap_or_default();
        let rel = self.store.get_value(&it, COL_REL).get::<String>().unwrap_or_default();
        if kind == "d" && !rel.is_empty() {
            if self.closed.contains(&rel) {
                self.view.expand_row(tpath, false); // children are pre-loaded (non-lazy store)
            } else {
                self.view.collapse_row(tpath);
            }
        } else if kind == "f" {
            let letter = self.store.get_value(&it, COL_LETTER).get::<String>().unwrap_or_default();
            let _ = sender.output(GitViewOutput::Open { rel, letter }); // (rel, letter) — chip needs the letter
        }
    }

    fn tog_w(&self) -> i32 {
        if let Some(w) = self.tog_w.get() {
            return w;
        }
        // python: try get_preferred_width(view)[1] except → 24; a natural of 0
        // (unrealized view) is that same failure
        let (min, natural) = self.tog.preferred_width(&self.view);
        let w = if natural > 0 { natural } else if min > 0 { min } else { 24 };
        self.tog_w.set(Some(w));
        w
    }

    // ---- commit+push (spec §4: the worker is a spawned thread; GTK via the
    //      command channel's receiver — relm4's idle marshalling) ---------------

    fn commit_clicked(&mut self, sender: &ComponentSender<Self>) {
        if self.busy || self.root.is_none() {
            return; // guard (python-exact)
        }
        let paths: Vec<String> = self.checked.iter().cloned().collect(); // BTreeSet = sorted
        if paths.is_empty() && self.ahead <= 0 {
            let _ = sender.output(GitViewOutput::Status(
                GitStatusKind::Err,
                "No files selected".into(),
            ));
            return;
        }
        let msg = self.msg.text().trim().to_string(); // GString borrow ends here
        if !paths.is_empty() && msg.is_empty() {
            let _ = sender.output(GitViewOutput::Status(
                GitStatusKind::Err,
                "Empty commit message".into(),
            ));
            return;
        }
        // root SNAPSHOT now (review I4): a SetRoot mid-flight must not reroute it
        let root = self.root.clone().unwrap();
        self.busy = true; // EAGER — buttons dead before the editor flush loop returns
        self.buttons();
        let _ = sender.output(GitViewOutput::Status(
            GitStatusKind::Busy,
            if paths.is_empty() { "Pushing…" } else { "Committing…" }.into(),
        ));
        let _ = sender.output(GitViewOutput::Busy(true));
        let _ = sender.output(GitViewOutput::Commit { root, paths, msg });
        // → editor SaveOpen → Flushed → StartCommit (app-side chain, T7)
    }

    fn start_commit(
        &mut self,
        root: PathBuf,
        paths: Vec<String>,
        msg: String,
        sender: &ComponentSender<Self>,
    ) {
        if self.flight {
            return; // one flight at a time (python's thread guard)
        }
        self.flight = true;
        sender.spawn_command(move |out| {
            let out_f = if paths.is_empty() {
                // push-only: python passes self._phase if paths else None
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
        v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
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
        let rows = rows_of(&[("z.md", "M"), ("a/b/c.md", "A")]); // unsorted on purpose
        assert_eq!(filtered(&rows, ""), rows, "empty needle → identity");
        assert_eq!(filtered(&rows, "c.md")[0].0, "a/b/c.md", "basename match");
        assert_eq!(filtered(&rows, "A/b")[0].0, "a/b/c.md", "rel match, case-insensitive");
        let both = filtered(&rows, "md");
        assert_eq!(both.len(), 2, "both names carry md");
        assert!(filtered(&rows, "zzz").is_empty());
    }

    #[test]
    fn toggled_checked_laws() {
        let vis = BTreeSet::from([
            "a/c.txt".to_string(), "a/d/m.md".to_string(), "e.md".to_string(),
        ]);
        let ck = BTreeSet::from(["a/c.txt".to_string(), "a/d/m.md".to_string()]);
        // file: flip
        let c = toggled_checked("f", "a/d/m.md", &ck, &vis).unwrap();
        assert!(!c.contains("a/d/m.md") && c.contains("a/c.txt"));
        // dir, none of its (visible) descendants selected → select all under it
        // (prefix = rel + "/", python gitview.py _on_toggled)
        let ck2 = BTreeSet::from(["a/c.txt".to_string()]);
        let c = toggled_checked("d", "a/d", &ck2, &vis).unwrap();
        assert!(c.contains("a/d/m.md"), "none selected → add all under dir");
        // dir all selected → clear under it
        let full = BTreeSet::from(["a/c.txt".to_string(), "a/d/m.md".to_string(),
                                   "e.md".to_string()]);
        let c = toggled_checked("d", "a", &full, &vis).unwrap();
        // python law is `checked -= files` (files = visible descendants of the
        // dir) — non-descendants SURVIVE: a/ clears a/c.txt + a/d/m.md, e.md stays
        assert_eq!(c, BTreeSet::from(["e.md".to_string()]), "descendants cleared, rest kept");
        // masthead (visible-only law, I2)
        let c = toggled_checked("s", "", &full, &vis).unwrap();
        assert!(c.is_empty());
        let c = toggled_checked("s", "", &BTreeSet::new(), &vis).unwrap();
        assert_eq!(c, vis);
        // dir with no visible descendants → |= ∅ no-op (Some, unchanged —
        // python's else-branch `checked |= set()`)
        let c = toggled_checked("d", "zzz", &ck, &vis).unwrap();
        assert_eq!(c, ck);
        // unknown kind → None (never flips a "No changes" row)
        assert!(toggled_checked("e", "x", &ck, &vis).is_none());
    }

    #[test]
    fn strip_icons_law() {
        // seti swap (2026-10-09): every file maps to a baked glyph (seti.rs cascade,
        // default = seti-default-white.svg) — "no-icon" only happens when the
        // svg rasterize FAILS, which is what the symbolic fallback guards.
        // This is the old no-icon bug's inverse: .rs rows now badge themselves.
        assert_eq!(strip_icons("editor.rs", false).1, None);
        assert_eq!(strip_icons("editor.rs", false).0.is_some(), true);
        assert_eq!(strip_icons("LICENSE", false).1, None); // seti license glyph, not fallback
        assert_eq!(strip_icons("whatever.zzz", false).0.is_some(), true); // default-white
        // dirs: folder art → no fallback
        assert_eq!(strip_icons("src", true).1, None);
    }
}
