// Right panel: search + lazily expanded file tree + status row (file count; git
// widgets land in S2's GitPanel).
//
// Tree invariant (measured on the python build, carried over): never mutate the
// store under an EXPANDED row — the child fill that used to run inside
// row-expanded made the view collapse itself. Children load from disk on every
// activation-expand of a collapsed row, before expand_row fires. Open state
// lives in `_open` (signal transitions are truthful; the view's row_expanded()
// readback is deprecated and stale after mutations).

// The module's whole API surface is the TreeModel/CellRenderer stack the python
// app uses — deprecated-since-4.10, functional and unpinned by gtk-rs replacement
// APIs. One blanket allow; Task 9's sweep re-runs the warning check.
#![allow(deprecated)]

use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use relm4::gtk::{self, gio, glib, pango, gdk_pixbuf, prelude::*};
use relm4::prelude::*;

use crate::badges;

pub const SKIP_DIRS: [&str; 6] = ["node_modules", ".git", "obj", "bin", "vendor", "__pycache__"];

// ext → themed mime icon (Papirus/Breeze names; anything else falls back)
fn ext_of(name: &str) -> String {
    name.rsplit('.').next().unwrap_or("").to_lowercase()
}

pub fn icon_of(name: &str, is_dir: bool) -> &'static str {
    if is_dir {
        return "folder-symbolic";
    }
    match ext_of(name).as_str() {
        "py" => "text-x-python",
        "json" => "application-json",
        "md" => "text-x-markdown",
        _ => "text-x-generic-symbolic",
    }
}

pub fn scan_dir(path: &Path) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let rd = match std::fs::read_dir(path) {
        Ok(rd) => rd,
        Err(_) => return out, // vanished workspace: empty, like python's OSError swallow
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false); // OSError → skipped row
        out.push((name, is_dir));
    }
    out.sort_by(|a, b| {
        (!a.1).cmp(&(!b.1)).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
    });
    out
}

/// Full walk, skip-list honored; filename-substring matches as sorted relpaths.
pub fn scan_project(root: &Path, needle: &str, cap: usize) -> Vec<String> {
    let mut m = Vec::new();
    walk(root, root, needle, cap, &mut m);
    m.sort();
    m
}

// ponytail: full walk; index/cache if trees get huge (matches python's own note)
fn walk(base: &Path, root: &Path, needle: &str, cap: usize, m: &mut Vec<String>) {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    match std::fs::read_dir(base) {
        Ok(rd) => {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if SKIP_DIRS.contains(&name.as_str()) {
                    continue;
                }
                match e.file_type() {
                    Ok(t) if t.is_dir() => dirs.push(name),
                    Ok(_) => files.push(name),
                    Err(_) => {}
                }
            }
        }
        Err(_) => return,
    }
    // os.walk order: base's files are matched before the walk descends —
    // the cap truncates the same subset of matches python's does
    files.sort();
    let lower = needle.to_lowercase();
    for f in files {
        let contains = f.to_lowercase().contains(&lower); // "" is in everything, like python
        if contains {
            let abs = base.join(&f);
            let rel = abs.strip_prefix(root).unwrap_or(&abs).to_string_lossy().replace('\\', "/");
            m.push(rel);
            if m.len() >= cap {
                return;
            }
        }
    }
    for d in dirs {
        walk(&base.join(&d), root, needle, cap, m);
        if m.len() >= cap {
            return;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("alpaca-tree-{tag}"));
        let _ = std::fs::remove_dir_all(&p); // stale scratch from a prior run: fresh each time
        p
    }

    #[test]
    fn scan_dir_skips_and_sorts_dirs_first() {
        let root = scratch("scan"); std::fs::create_dir_all(&root).unwrap();
        for d in ["node_modules", "src"] { std::fs::create_dir(root.join(d)).unwrap(); }
        for f in ["b.rs", "a.rs", "z.py"] { std::fs::write(root.join(f), "").unwrap(); }
        let got = scan_dir(&root);
        let names: Vec<&str> = got.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["src", "a.rs", "b.rs", "z.py"]); // node_modules skipped; dirs first
        assert!(got.iter().all(|(_, is_dir)| *is_dir || names.iter().any(|n| !n.is_empty())));
    }

    #[test]
    fn scan_project_matches_and_skips() {
        let root = scratch("walk"); let src = root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir(root.join("node_modules")).unwrap();
        std::fs::write(src.join("main.rs"), "").unwrap();
        std::fs::write(root.join("README.md"), "").unwrap();
        std::fs::write(root.join("node_modules").join("junk.rs"), "").unwrap();
        let got = scan_project(&root, "rs", 500);
        assert_eq!(got, vec!["src/main.rs".to_string()]); // node_modules pruned
        let n = scan_project(&root, "", 100_000).len();
        assert_eq!(n, 2); // count path also skips junk
    }

    #[test]
    fn icon_of_rules() {
        assert_eq!(icon_of("main.py", true), "folder-symbolic");
        assert_eq!(icon_of("a.py", false), "text-x-python");
        assert_eq!(icon_of("a.json", false), "application-json");
        assert_eq!(icon_of("a.md", false), "text-x-markdown");
        assert_eq!(icon_of("a.xyz", false), "text-x-generic-symbolic");
    }
}

// ---- HoverTree (treehover.py) — hover-band TreeView -----------------------
//
// GTK4 tree rows expose no :hover state: gtktreeview folds the widget's own
// state into EVERY row's background paint — a `treeview:hover` CSS rule would
// tint the whole widget. Pointer position is tracked instead, kept as a
// TreeRowReference (stable across model shifts above the row; `valid()` goes
// false when the row is deleted), painted in the snapshot vfunc BEFORE the
// chain-up — rows' transparent CSS background lets the band show, while
// opaque paint (selected #1b3560, cells' text) draws on top of it.

use crate::treehover::HoverTree;


// ---- FileTree component (filetree.py:64-541 minus git) --------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirEv {
    Created,
    Deleted,
    Moved,
}

fn dir_of_event(e: gio::FileMonitorEvent) -> Option<DirEv> {
    match e {
        gio::FileMonitorEvent::Created => Some(DirEv::Created),
        gio::FileMonitorEvent::Deleted => Some(DirEv::Deleted),
        gio::FileMonitorEvent::Moved => Some(DirEv::Moved),
        _ => None,
    }
}

#[derive(Debug)]
pub enum FileTreeMsg {
    SetRoot(PathBuf),
    Search(String),
    RowExpanded(gtk::TreePath),
    RowCollapsed(gtk::TreePath),
    Activated(gtk::TreePath),
    /// dir monitor event (Created/Deleted/Moved only), parent = abs dir path
    DirChanged { parent: String, file: Option<PathBuf>, ev: DirEv },
    /// search-clear restore: replay open dirs parents-first, then scroll
    RestoreTree(PathBuf, Vec<String>, f64),
    /// store mutated → re-evaluate the hover band under a stationary pointer
    RefreshHover,
}

#[derive(Debug)]
pub enum FileTreeOutput {
    OpenFile(PathBuf),
    /// file count for GitPanel's status row (the label left with the chrome)
    Count(usize),
}

pub struct FileTree {
    root: Option<PathBuf>,
    store: gtk::TreeStore,
    view: HoverTree,
    open: HashSet<String>,       // _open: currently expanded abs paths
    monitors: Vec<gio::FileMonitor>,
    monitored: HashSet<String>,  // dirs with a live monitor (cap)
    saved_tree: Option<(PathBuf, HashSet<String>, f64)>,
}

#[relm4::component(pub)]
impl Component for FileTree {
    type CommandOutput = ();
    type Input = FileTreeMsg;
    type Output = FileTreeOutput;
    type Init = ();

    view! {
        #[root]
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
        }
    }

    fn init(_: (), root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        // card chrome (head/search/status row) moved to GitPanel — the filetree
        // is the tree wrapper inside the panel's stack ("tree" child)

        // display, abs path, is_dir, symbolic fallback icon, chevron pixbuf,
        // mock badge, badge/icon visible
        let store = gtk::TreeStore::new(&[
            glib::Type::STRING,  // 0 name
            glib::Type::STRING,  // 1 abs path
            glib::Type::BOOL,    // 2 is_dir
            glib::Type::STRING,  // 3 symbolic fallback icon
            gdk_pixbuf::Pixbuf::static_type(), // 4 chevron
            gdk_pixbuf::Pixbuf::static_type(), // 5 badge
            glib::Type::BOOL,    // 6 badge visible
            glib::Type::BOOL,    // 7 icon visible
        ]);

        let view = HoverTree::default();
        view.set_model(Some(&store));
        view.set_headers_visible(false);
        view.set_activate_on_single_click(true); // single click = open/toggle
        #[allow(deprecated)] // gtk_tree_view_set_show_expanders deprecated 4.10; python calls it too
        view.set_show_expanders(false);          // our own chevron cell, mock-style
        view.set_tooltip_column(1);              // ellipsized names: hover shows the full path
        view.set_level_indentation(16);          // indent: 8 + 16·depth
        view.set_css_classes(&["alpaca-tree"]);

        // cells packed in python's order (filetree.py:137-161); text cell LAST,
        // expand=True — pins chevron/badge/icon left against the indentation
        #[allow(deprecated)] // CellRenderers: live, unused-deprecation noise on this build
        let pix_chev = gtk::CellRendererPixbuf::new();
        pix_chev.set_property("xpad", 2u32);
        pix_chev.set_property("ypad", 2u32);
        pix_chev.set_property("yalign", 2.0f32 / 3.0f32); // chevron rides 4/6, dir rows stay flush
        #[allow(deprecated)]
        let pix_badge = gtk::CellRendererPixbuf::new();
        pix_badge.set_property("xpad", 2u32);
        pix_badge.set_property("ypad", 2u32);
        pix_badge.set_property("yalign", 1.0f32); // bottom-pin: pixbuf cells center
        #[allow(deprecated)]                      // on the line box while ink hangs ~1px below
        let pix_icon = gtk::CellRendererPixbuf::new();
        pix_icon.set_property("xpad", 2u32);
        pix_icon.set_property("ypad", 2u32);
        pix_icon.set_property("yalign", 1.0f32);
        #[allow(deprecated)]
        let cell = gtk::CellRendererText::new();
        cell.set_property("xpad", 2u32);
        cell.set_property("ypad", 2u32);
        cell.set_property("ellipsize", pango::EllipsizeMode::Middle); // before any insert
        let col = gtk::TreeViewColumn::new();
        col.pack_start(&pix_chev, false);
        col.add_attribute(&pix_chev, "pixbuf", 4);
        col.pack_start(&pix_badge, false);
        col.add_attribute(&pix_badge, "pixbuf", 5);
        col.add_attribute(&pix_badge, "visible", 6);
        col.pack_start(&pix_icon, false);
        col.add_attribute(&pix_icon, "icon-name", 3);
        col.add_attribute(&pix_icon, "visible", 7);
        col.pack_start(&cell, true); // text last, expand
        col.add_attribute(&cell, "text", 0);
        view.append_column(&col);
        let tree_page = gtk::ScrolledWindow::builder()
            .child(&view)
            .vexpand(true)
            .build();
        root.append(&tree_page);

        // signals
        let snd = sender.clone();
        view.connect_row_expanded(move |_, _, path| {
            snd.input(FileTreeMsg::RowExpanded(path.clone()));
        });
        let snd = sender.clone();
        view.connect_row_collapsed(move |_, _, path| {
            snd.input(FileTreeMsg::RowCollapsed(path.clone()));
        });
        let snd = sender.clone();
        view.connect_row_activated(move |_, path, _| {
            snd.input(FileTreeMsg::Activated(path.clone()));
        });
        // hover band must follow store mutations (rows inserted/deleted while
        // the pointer stays put) — refresh through the view's own path logic
        store.connect_row_inserted({
            let snd = sender.clone();
            move |_, _, _| snd.input(FileTreeMsg::RefreshHover)
        });
        store.connect_row_deleted({
            let snd = sender.clone();
            move |_, _| snd.input(FileTreeMsg::RefreshHover)
        });

        let model = FileTree {
            root: None,
            store: store.clone(),
            view: view.clone(),
            open: HashSet::new(),
            monitors: Vec::new(),
            monitored: HashSet::new(),
            saved_tree: None,
        };
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            FileTreeMsg::SetRoot(path) => self.set_root(path, &sender),
            FileTreeMsg::Search(text) => self.search(&text, &sender),
            FileTreeMsg::RowExpanded(tpath) => self.row_expanded(&tpath, &sender),
            FileTreeMsg::RowCollapsed(tpath) => self.row_collapsed(&tpath),
            FileTreeMsg::Activated(tpath) => self.activated(&tpath, &sender),
            FileTreeMsg::DirChanged { parent, file, ev } => {
                self.dir_changed(&parent, file.as_ref(), ev, &sender)
            }
            FileTreeMsg::RestoreTree(root, open, scroll) => {
                self.restore_tree(&root, open, scroll)
            }
            FileTreeMsg::RefreshHover => self.view.refresh_hover(),
        }
    }
}

// store column indexes
const NAME: i32 = 0;
const PATH: i32 = 1;
const IS_DIR: i32 = 2;

impl FileTree {    // ---- store helpers ---------------------------------------------------------

    fn row_str(&self, it: &gtk::TreeIter, col: i32) -> String {
        self.store.get_value(it, col).get::<String>().unwrap_or_default()
    }

    fn row_bool(&self, it: &gtk::TreeIter, col: i32) -> bool {
        self.store.get_value(it, col).get::<bool>().unwrap_or(false)
    }

    fn row_child(&self, parent: Option<&gtk::TreeIter>, name: &str) -> Option<gtk::TreeIter> {
        let mut it = self.store.iter_children(parent)?;
        loop {
            if self.row_str(&it, NAME) == name {
                return Some(it);
            }
            if !self.store.iter_next(&mut it) {
                return None;
            }
        }
    }


    /// One store row: chevron slot for dirs (blank for files), badge pixbuf
    /// when chip art exists (else the symbolic icon cell takes col 7).
    fn push_row(&self, parent: Option<&gtk::TreeIter>, name: &str, path: &str, is_dir: bool) {
        let pix = badges::pixbuf_for(name, is_dir);
        let chev = if is_dir {
            badges::chevron_pixbuf(false).unwrap_or_else(badges::blank_pixbuf)
        } else {
            badges::blank_pixbuf()
        };
        let icon = icon_of(name, is_dir);
        let name_v = name.to_string();
        let path_v = path.to_string();
        let mut vals: Vec<(u32, &dyn glib::value::ToValue)> = vec![
            (0, &name_v),
            (1, &path_v),
            (2, &is_dir),
            (3, &icon),
            (4, &chev),
        ];
        let badge_vis = pix.is_some();
        let icon_vis = pix.is_none();
        if let Some(p) = &pix {
            vals.push((5, p));
        }
        vals.push((6, &badge_vis));
        vals.push((7, &icon_vis));
        #[allow(deprecated)]
        self.store.insert_with_values(parent, None, &vals);
    }

    // ---- root / population ---------------------------------------------------

    /// store.clear + fresh scan rows; monitor/expansion state resets with the
    /// rows they pointed at (filetree.py:_populate_root).
    fn populate_root(&mut self) {
        self.store.clear();
        self.monitored.clear();
        self.open.clear();
        if let Some(root) = self.root.clone() {
            for (name, is_dir) in scan_dir(&root) {
                let p = root.join(&name);
                self.push_row(None, &name, &p.to_string_lossy(), is_dir);
            }
        }
        self.view.refresh_hover();
    }

    /// full count → the panel routes it to the status row (Count output)
    fn count(&self, sender: &ComponentSender<Self>) {
        let n = self
            .root
            .as_ref()
            .map(|r| scan_project(r, "", 100_000).len())
            .unwrap_or(0);
        let _ = sender.output(FileTreeOutput::Count(n));
    }

    fn set_root(&mut self, root: PathBuf, sender: &ComponentSender<Self>) {
        for m in &self.monitors {
            m.cancel();
        }
        self.monitors.clear();
        self.root = Some(root);
        self.saved_tree = None; // dead before the panel's queued Search("") lands
        // the panel clears its entry now (its queued Search("") repeats are
        // idempotent); populate + count stay explicit
        self.populate_root();
        self.count(sender);
    }

    // ---- search ---------------------------------------------------------------

    fn search(&mut self, text: &str, sender: &ComponentSender<Self>) {
        let text = text.trim().to_string();
        let Some(root) = self.root.clone() else { return };
        if text.is_empty() {
            self.populate_root();
            if let Some((sroot, sopen, s)) = self.saved_tree.take() {
                if sroot == root && !sopen.is_empty() {
                    let snd2 = sender.clone();
                    glib::idle_add_local_once(move || {
                        snd2.input(FileTreeMsg::RestoreTree(root, sopen.into_iter().collect(), s))
                    });
                }
            }
            return;
        }
        if self.saved_tree.is_none() {
            self.saved_tree = Some((root.clone(), self.open.clone(), self.view.vadjustment().map(|a| a.value()).unwrap_or(0.0)));
        }
        self.store.clear();
        self.open.clear(); // search view has no tree rows (monitors stay; open gate eats their events)
        self.monitored.clear();
        for rel in scan_project(&root, &text, 500) {
            let p = root.join(&rel);
            self.push_row(None, &rel, &p.to_string_lossy(), false);
        }
        self.view.refresh_hover();
    }

    // ---- search-clear restore (idle stages) ---------------------------------------

    /// Stage 1: replay remembered open dirs parents-first (shallow→deep), then
    /// hand the scroll to stage 2. A workspace opened meanwhile cancels silently.
    fn restore_tree(&mut self, root: &PathBuf, open: Vec<String>, scroll: f64) {
        if self.root.as_deref() != Some(root.as_path()) {
            return;
        }
        let mut sorted = open;
        sorted.sort_by_key(|p| p.matches('/').count());
        for d in &sorted {
            self.reexpand(d);
        }
        let view = self.view.clone();
        glib::idle_add_local_once(move || {
            // clamp: scroll restore must not overshoot a shorter tree
            if let Some(adj) = view.vadjustment() {
                adj.set_value(scroll.min((adj.upper() - adj.page_size()).max(0.0)));
            }
        });
    }

    fn reexpand(&mut self, d: &str) {
        let Some(root) = self.root.as_ref() else { return };
        let Ok(rel) = Path::new(d).strip_prefix(root) else { return };
        let mut parent: Option<gtk::TreeIter> = None;
        for seg in rel.iter() {
            let seg = seg.to_string_lossy();
            let Some(i) = self.row_child(parent.as_ref(), &seg) else { return };
            let dd = self.row_str(&i, PATH);
            if self.store.iter_children(Some(&i)).is_none() {
                self.load_children(&i, &dd); // lazy level: load, then expand
            }
            self.view.expand_row(&self.store.path(&i), false);
            parent = Some(i);
        }
    }

    // ---- lazy children -------------------------------------------------------------

    /// Replace collapsed parent's children with a fresh scan from disk.
    /// Collapsed-row mutations only — under an expanded row this collapses it.
    fn load_children(&mut self, parent: &gtk::TreeIter, d: &str) {
        while let Some(c) = self.store.iter_children(Some(parent)) {
            self.store.remove(&c);
        }
        let under = format!("{d}/");
        // fresh rows are collapsed; remembered open-states under d are stale
        self.open.retain(|p| p == d || !p.starts_with(&under));
        let base = Path::new(d);
        for (name, is_dir) in scan_dir(base) {
            let p = base.join(&name);
            self.push_row(Some(parent), &name, &p.to_string_lossy(), is_dir);
        }
        self.view.refresh_hover();
    }

    // ---- expansion / monitors ----------------------------------------------------

    fn row_expanded(&mut self, tpath: &gtk::TreePath, sender: &ComponentSender<Self>) {
        let Some(it) = self.store.iter(tpath) else { return };
        let d = self.row_str(&it, PATH);
        if d.is_empty() {
            return;
        }
        self.open.insert(d.clone());
        let down = badges::chevron_pixbuf(true).unwrap_or_else(badges::blank_pixbuf);
        #[allow(deprecated)]
        self.store.set_value(&it, 4, &down.to_value());
        // monitors only for dirs we've actually shown the user (open once, cap)
        if self.row_bool(&it, IS_DIR)
            && !self.monitored.contains(&d)
            && self.monitors.len() < 50
        {
            self.monitored.insert(d.clone());
            if let Ok(mon) = gio::File::for_path(&d).monitor_directory(
                gio::FileMonitorFlags::NONE,
                gio::Cancellable::NONE,
            ) {
                let snd2 = sender.clone();
                mon.connect_changed(move |_m, f, _o, ev| {
                    if let Some(de) = dir_of_event(ev) {
                        snd2.input(FileTreeMsg::DirChanged {
                            parent: d.clone(),
                            file: f.path(),
                            ev: de,
                        });
                    }
                });
                self.monitors.push(mon);
            }
        }
    }

    fn row_collapsed(&mut self, tpath: &gtk::TreePath) {
        let Some(it) = self.store.iter(tpath) else { return };
        let d = self.row_str(&it, PATH);
        if !d.is_empty() {
            self.open.remove(&d);
        }
        let up = badges::chevron_pixbuf(false).unwrap_or_else(badges::blank_pixbuf);
        #[allow(deprecated)]
        self.store.set_value(&it, 4, &up.to_value());
    }

    // ---- dir monitor events ------------------------------------------------------

    /// CREATED/DELETED/MOVED only. count first, then the collapsed/open gates.
    fn dir_changed(&mut self, parent: &str, file: Option<&PathBuf>, ev: DirEv, sender: &ComponentSender<Self>) {
        self.count(sender);
        if !self.open.contains(parent) {
            return; // collapsed: next expansion reloads from disk anyway
        }
        let Some(p) = file else { return };
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else { return };
        if name.is_empty() || SKIP_DIRS.contains(&name) {
            return;
        }
        let is_dir = if ev == DirEv::Deleted {
            false
        } else {
            gio::File::for_path(p)
                .query_info(
                    "standard::type",
                    gio::FileQueryInfoFlags::NONE,
                    gio::Cancellable::NONE,
                )
                .map(|i| i.file_type() == gio::FileType::Directory)
                .unwrap_or(false)
        };
        // surgical: remove the file's rows under every instance, re-add the
        // new one unsorted (next collapse/re-expand restores scan order)
        for row_i in self.iters_for(parent) {
            let mut cur = self.store.iter_children(Some(&row_i));
            while let Some(ci) = cur {
                cur = {
                    let mut t = ci.clone();
                    if self.store.iter_next(&mut t) { Some(t) } else { None }
                };
                if self.row_str(&ci, PATH) == p.to_string_lossy() {
                    self.store.remove(&ci);
                }
            }
            if ev != DirEv::Deleted {
                self.push_row(Some(&row_i), name, &p.to_string_lossy(), is_dir);
            }
            self.view.refresh_hover();
        }
    }

    /// All iters whose row abs-path == path (same-named dirs may exist under
    /// several parents). Iters are cloned out before any mutation.
    fn iters_for(&self, path: &str) -> Vec<gtk::TreeIter> {
        let mut found = Vec::new();
        self.walk_iters(None, path, &mut found);
        found
    }

    fn walk_iters(&self, parent: Option<&gtk::TreeIter>, path: &str, found: &mut Vec<gtk::TreeIter>) {
        let mut cur = match parent {
            None => self.store.iter_first(),
            Some(p) => self.store.iter_children(Some(p)),
        };
        while let Some(i) = cur {
            cur = {
                let mut t = i.clone();
                if self.store.iter_next(&mut t) { Some(t) } else { None }
            };
            let is_dir = self.row_bool(&i, IS_DIR);
            if self.row_str(&i, PATH) == path {
                found.push(i.clone());
            }
            if is_dir {
                self.walk_iters(Some(&i), path, found);
            }
        }
    }

    // ---- activation --------------------------------------------------------------------

    /// activate_on_single_click is on: one click activates. Dir rows toggle
    /// expand/collapse; the open/close decision uses `open`, never the view's
    /// (deprecated) row_expanded() readback.
    fn activated(&mut self, tpath: &gtk::TreePath, sender: &ComponentSender<Self>) {
        let Some(it) = self.store.iter(tpath) else { return };
        if self.row_bool(&it, IS_DIR) {
            let d = self.row_str(&it, PATH);
            if self.open.contains(&d) {
                self.view.collapse_row(tpath);
            } else if !d.is_empty() {
                self.load_children(&it, &d);
                self.view.expand_row(tpath, false);
            }
        } else {
            let p = self.row_str(&it, PATH);
            if !p.is_empty() {
                let _ = sender.output(FileTreeOutput::OpenFile(PathBuf::from(p)));
            }
        }
    }
}
