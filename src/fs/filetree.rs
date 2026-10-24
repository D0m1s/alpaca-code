#![allow(deprecated)]

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use relm4::gtk::{self, gdk_pixbuf, gio, glib, pango, prelude::*};
use relm4::prelude::*;

use crate::git::gitstatus;
use crate::render::badges;

pub const SKIP_DIRS: [&str; 7] = [
    "node_modules",
    ".git",
    "obj",
    "bin",
    "vendor",
    "__pycache__",
    "target",
];

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

fn dominant_ext(names: Vec<String>) -> Option<String> {
    let mut tally: BTreeMap<String, u64> = BTreeMap::new();
    for n in names {
        let base = n.rsplit('/').next().unwrap_or(&n);
        if !base.contains('.') || base.starts_with('.') {
            continue;
        }
        let e = ext_of(base);
        if e.is_empty() {
            continue;
        }
        *tally.entry(e).or_insert(0) += 1;
    }
    let mut best: Option<(String, u64)> = None;
    for (e, n) in tally {
        if best.as_ref().map(|(_, bn)| n > *bn).unwrap_or(true) {
            best = Some((e, n));
        }
    }
    best.map(|(e, _)| e)
}

const PROSE_EXTS: [&str; 3] = ["md", "rst", "txt"];

const GENERATED_EXTS: [&str; 1] = ["lock"];
const GENERATED_NAMES: [&str; 2] = ["package-lock.json", "pnpm-lock.yaml"];

fn lang_of_ext(ext: &str) -> Option<String> {
    sourceview5::LanguageManager::default()
        .guess_language(Some(&format!("x.{ext}")), None)
        .map(|l| l.id().to_string())
}

pub fn dominant_language(root: &Path) -> Option<String> {
    dominant_ext_of(root).and_then(|e| lang_of_ext(&e))
}

pub fn dominant_ext_of(root: &Path) -> Option<String> {
    let root_s = root.to_string_lossy().to_string();
    let files = gitstatus::git_out(
        &root_s,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
        15,
    )
    .map(|out| {
        out.split(|b| *b == 0)
            .filter(|p| !p.is_empty())
            .map(|p| root.join(String::from_utf8_lossy(p).into_owned()))
            .collect::<Vec<PathBuf>>()
    });
    match files {
        Some(files) => max_ext_bytes(&files),
        None => dominant_ext(scan_project(root, "", 100_000)),
    }
}

fn max_ext_bytes(paths: &[PathBuf]) -> Option<String> {
    let mut tally: BTreeMap<String, u64> = BTreeMap::new();
    for p in paths {
        if let Ok(md) = p.metadata() {
            if md.is_dir() {
                continue;
            }
            let base = p.file_name().unwrap_or_default().to_string_lossy();
            if !base.contains('.') || base.starts_with('.') {
                continue;
            }
            if GENERATED_NAMES.contains(&base.to_lowercase().as_str()) {
                continue;
            }
            let e = ext_of(&base);
            if e.is_empty()
                || PROSE_EXTS.contains(&e.as_str())
                || GENERATED_EXTS.contains(&e.as_str())
            {
                continue;
            }
            *tally.entry(e).or_insert(0) += md.len();
        }
    }
    let mut best: Option<(String, u64)> = None;
    for (e, n) in tally {
        if best.as_ref().map(|(_, bn)| n > *bn).unwrap_or(true) {
            best = Some((e, n));
        }
    }
    best.map(|(e, _)| e)
}

pub fn scan_dir(path: &Path) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let rd = match std::fs::read_dir(path) {
        Ok(rd) => rd,
        Err(_) => return out,
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        out.push((name, is_dir));
    }
    out.sort_by(|a, b| {
        (!a.1)
            .cmp(&(!b.1))
            .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
    });
    out
}

pub fn scan_project(root: &Path, needle: &str, cap: usize) -> Vec<String> {
    let mut m = Vec::new();
    walk(root, root, needle, cap, &mut m);
    m.sort();
    m
}

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
    files.sort();
    let lower = needle.to_lowercase();
    for f in files {
        let contains = f.to_lowercase().contains(&lower);
        if contains {
            let abs = base.join(&f);
            let rel = abs
                .strip_prefix(root)
                .unwrap_or(&abs)
                .to_string_lossy()
                .replace('\\', "/");
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
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    #[test]
    fn scan_dir_skips_and_sorts_dirs_first() {
        let root = scratch("scan");
        std::fs::create_dir_all(&root).unwrap();
        for d in ["node_modules", "src"] {
            std::fs::create_dir(root.join(d)).unwrap();
        }
        for f in ["b.rs", "a.rs", "z.py"] {
            std::fs::write(root.join(f), "").unwrap();
        }
        let got = scan_dir(&root);
        let names: Vec<&str> = got.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["src", "a.rs", "b.rs", "z.py"]);
        assert!(
            got.iter()
                .all(|(_, is_dir)| *is_dir || names.iter().any(|n| !n.is_empty()))
        );
    }

    #[test]
    fn scan_project_matches_and_skips() {
        let root = scratch("walk");
        let src = root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir(root.join("node_modules")).unwrap();
        std::fs::write(src.join("main.rs"), "").unwrap();
        std::fs::write(root.join("README.md"), "").unwrap();
        std::fs::write(root.join("node_modules").join("junk.rs"), "").unwrap();
        let got = scan_project(&root, "rs", 500);
        assert_eq!(got, vec!["src/main.rs".to_string()]);
        let n = scan_project(&root, "", 100_000).len();
        assert_eq!(n, 2);
    }

    #[test]
    fn dominant_ext_fold() {
        let got = dominant_ext(vec![
            "a.rs".into(),
            "src/b.rs".into(),
            "c/py/x.py".into(),
            "d.rs".into(),
            "e.py".into(),
        ]);
        assert_eq!(got.as_deref(), Some("rs"));
        assert_eq!(dominant_ext(vec!["Makefile".into(), "x".into()]), None);
        let tie = dominant_ext(vec!["z.rs".into(), "a.py".into()]);
        assert_eq!(tie.as_deref(), Some("py"));
        assert_eq!(
            dominant_ext(vec![
                ".gitignore".into(),
                "src/.env".into(),
                ".editorconfig".into()
            ]),
            None
        );
        assert_eq!(dominant_ext(Vec::new()), None);
    }

    fn run_git(args: &[&str], cwd: &Path) {
        let st = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(std::process::Stdio::null())
            .output()
            .expect("git binary available");
        assert!(
            st.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&st.stderr)
        );
    }

    #[test]
    fn dominant_language_git_weights_bytes_and_skips_ignored() {
        let root = scratch("dom-git");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("main.rs"), "x".repeat(40_000)).unwrap();
        std::fs::write(root.join("a.svg"), "<svg/>".repeat(8)).unwrap();
        std::fs::write(root.join("README.md"), "y".repeat(200_000)).unwrap();
        std::fs::write(root.join("package-lock.json"), "j".repeat(500_000)).unwrap();
        std::fs::write(root.join("Cargo.lock"), "c".repeat(300_000)).unwrap();
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(root.join("target/o.o"), "z".repeat(300_000)).unwrap();
        std::fs::write(root.join("big.log"), "l".repeat(300_000)).unwrap();
        std::fs::write(root.join(".gitignore"), "target/\n*.log\n").unwrap();
        run_git(&["init", "-q"], &root);
        run_git(&["add", "."], &root);
        assert_eq!(dominant_ext_of(&root).as_deref(), Some("rs"));
    }

    #[test]
    fn dominant_language_walk_fallback_without_git() {
        let root = scratch("dom-walk");
        std::fs::create_dir_all(&root).unwrap();
        for i in 0..5 {
            std::fs::write(root.join(format!("m{i}.py")), "p").unwrap();
        }
        std::fs::write(root.join("big.rs"), "x".repeat(50_000)).unwrap();
        assert_eq!(dominant_ext_of(&root).as_deref(), Some("py"));
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

use crate::fs::treehover::HoverTree;

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
    DirChanged {
        parent: String,
        file: Option<PathBuf>,
        ev: DirEv,
    },
    RestoreTree(PathBuf, Vec<String>, f64),
    CountTick(u64),
    RefreshHover,
}

#[derive(Debug)]
pub enum FileTreeOutput {
    OpenFile(PathBuf),
    Count(usize),
}

pub struct FileTree {
    root: Option<PathBuf>,
    store: gtk::TreeStore,
    view: HoverTree,
    open: HashSet<String>,
    monitors: Vec<gio::FileMonitor>,
    monitored: HashSet<String>,
    saved_tree: Option<(PathBuf, HashSet<String>, f64)>,
    count_gen: u64,
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
        let store = gtk::TreeStore::new(&[
            glib::Type::STRING,
            glib::Type::STRING,
            glib::Type::BOOL,
            glib::Type::STRING,
            gdk_pixbuf::Pixbuf::static_type(),
            gdk_pixbuf::Pixbuf::static_type(),
            glib::Type::BOOL,
            glib::Type::BOOL,
        ]);

        let view = HoverTree::default();
        view.set_model(Some(&store));
        view.set_headers_visible(false);
        view.set_activate_on_single_click(true);
        #[allow(deprecated)]
        view.set_show_expanders(false);
        view.set_tooltip_column(1);
        view.set_level_indentation(16);
        view.set_css_classes(&["alpaca-tree"]);

        #[allow(deprecated)]
        let pix_chev = gtk::CellRendererPixbuf::new();
        pix_chev.set_property("xpad", 2u32);
        pix_chev.set_property("ypad", 2u32);
        pix_chev.set_property("yalign", 2.0f32 / 3.0f32);
        #[allow(deprecated)]
        let pix_badge = gtk::CellRendererPixbuf::new();
        pix_badge.set_property("xpad", 2u32);
        pix_badge.set_property("ypad", 2u32);
        pix_badge.set_property("yalign", 1.0f32);
        #[allow(deprecated)]
        let pix_icon = gtk::CellRendererPixbuf::new();
        pix_icon.set_property("xpad", 2u32);
        pix_icon.set_property("ypad", 2u32);
        pix_icon.set_property("yalign", 1.0f32);
        #[allow(deprecated)]
        let cell = gtk::CellRendererText::new();
        cell.set_property("xpad", 2u32);
        cell.set_property("ypad", 2u32);
        cell.set_property("ellipsize", pango::EllipsizeMode::Middle);
        let col = gtk::TreeViewColumn::new();
        col.pack_start(&pix_chev, false);
        col.add_attribute(&pix_chev, "pixbuf", 4);
        col.pack_start(&pix_badge, false);
        col.add_attribute(&pix_badge, "pixbuf", 5);
        col.add_attribute(&pix_badge, "visible", 6);
        col.pack_start(&pix_icon, false);
        col.add_attribute(&pix_icon, "icon-name", 3);
        col.add_attribute(&pix_icon, "visible", 7);
        col.pack_start(&cell, true);
        col.add_attribute(&cell, "text", 0);
        view.append_column(&col);
        let tree_page = gtk::ScrolledWindow::builder()
            .child(&view)
            .vexpand(true)
            .build();
        root.append(&tree_page);

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
            count_gen: 0,
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
            FileTreeMsg::CountTick(g) => {
                if g == self.count_gen {
                    self.count(&sender);
                }
            }
            FileTreeMsg::RestoreTree(root, open, scroll) => self.restore_tree(&root, open, scroll),
            FileTreeMsg::RefreshHover => self.view.refresh_hover(),
        }
    }
}

const NAME: i32 = 0;
const PATH: i32 = 1;
const IS_DIR: i32 = 2;

impl FileTree {
    pub fn open_dirs(&self) -> Vec<String> {
        self.open.iter().cloned().collect()
    }

    fn row_str(&self, it: &gtk::TreeIter, col: i32) -> String {
        self.store
            .get_value(it, col)
            .get::<String>()
            .unwrap_or_default()
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
        self.saved_tree = None;
        self.populate_root();
        self.count(sender);
    }

    fn search(&mut self, text: &str, sender: &ComponentSender<Self>) {
        let text = text.trim().to_string();
        let Some(root) = self.root.clone() else {
            return;
        };
        if text.is_empty() {
            self.populate_root();
            if let Some((sroot, sopen, s)) = self.saved_tree.take()
                && sroot == root
                && !sopen.is_empty()
            {
                let snd2 = sender.clone();
                glib::idle_add_local_once(move || {
                    let _ = snd2.input_sender().send(FileTreeMsg::RestoreTree(
                        root,
                        sopen.into_iter().collect(),
                        s,
                    ));
                });
            }
            return;
        }
        if self.saved_tree.is_none() {
            self.saved_tree = Some((
                root.clone(),
                self.open.clone(),
                self.view.vadjustment().map(|a| a.value()).unwrap_or(0.0),
            ));
        }
        self.store.clear();
        self.open.clear();
        self.monitored.clear();
        for rel in scan_project(&root, &text, 500) {
            let p = root.join(&rel);
            self.push_row(None, &rel, &p.to_string_lossy(), false);
        }
        self.view.refresh_hover();
    }

    fn restore_tree(&mut self, root: &Path, open: Vec<String>, scroll: f64) {
        if self.root.as_deref() != Some(root) {
            return;
        }
        let mut sorted = open;
        sorted.sort_by_key(|p| p.matches('/').count());
        for d in &sorted {
            self.reexpand(d);
        }
        let view = self.view.clone();
        glib::idle_add_local_once(move || {
            if let Some(adj) = view.vadjustment() {
                adj.set_value(scroll.min((adj.upper() - adj.page_size()).max(0.0)));
            }
        });
    }

    fn reexpand(&mut self, d: &str) {
        let Some(root) = self.root.as_ref() else {
            return;
        };
        let Ok(rel) = Path::new(d).strip_prefix(root) else {
            return;
        };
        let mut parent: Option<gtk::TreeIter> = None;
        for seg in rel.iter() {
            let seg = seg.to_string_lossy();
            let Some(i) = self.row_child(parent.as_ref(), &seg) else {
                return;
            };
            let dd = self.row_str(&i, PATH);
            if self.store.iter_children(Some(&i)).is_none() {
                self.load_children(&i, &dd);
            }
            self.view.expand_row(&self.store.path(&i), false);
            parent = Some(i);
        }
    }

    fn load_children(&mut self, parent: &gtk::TreeIter, d: &str) {
        while let Some(c) = self.store.iter_children(Some(parent)) {
            self.store.remove(&c);
        }
        let under = format!("{d}/");
        self.open.retain(|p| p == d || !p.starts_with(&under));
        let base = Path::new(d);
        for (name, is_dir) in scan_dir(base) {
            let p = base.join(&name);
            self.push_row(Some(parent), &name, &p.to_string_lossy(), is_dir);
        }
        self.view.refresh_hover();
    }

    fn row_expanded(&mut self, tpath: &gtk::TreePath, sender: &ComponentSender<Self>) {
        let Some(it) = self.store.iter(tpath) else {
            return;
        };
        let d = self.row_str(&it, PATH);
        if d.is_empty() {
            return;
        }
        self.open.insert(d.clone());
        let down = badges::chevron_pixbuf(true).unwrap_or_else(badges::blank_pixbuf);
        #[allow(deprecated)]
        self.store.set_value(&it, 4, &down.to_value());
        if self.row_bool(&it, IS_DIR) && !self.monitored.contains(&d) && self.monitors.len() < 50 {
            self.monitored.insert(d.clone());
            if let Ok(mon) = gio::File::for_path(&d)
                .monitor_directory(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
            {
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
        let Some(it) = self.store.iter(tpath) else {
            return;
        };
        let d = self.row_str(&it, PATH);
        if !d.is_empty() {
            self.open.remove(&d);
        }
        let up = badges::chevron_pixbuf(false).unwrap_or_else(badges::blank_pixbuf);
        #[allow(deprecated)]
        self.store.set_value(&it, 4, &up.to_value());
    }

    fn dir_changed(
        &mut self,
        parent: &str,
        file: Option<&PathBuf>,
        ev: DirEv,
        sender: &ComponentSender<Self>,
    ) {
        self.count_gen += 1;
        let g = self.count_gen;
        let snd = sender.clone();
        glib::timeout_add_local(Duration::from_millis(300), move || {
            if snd.input_sender().send(FileTreeMsg::CountTick(g)).is_err() {
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Break
        });
        if !self.open.contains(parent) {
            return;
        }
        let Some(p) = file else { return };
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
            return;
        };
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
        for row_i in self.iters_for(parent) {
            let mut cur = self.store.iter_children(Some(&row_i));
            while let Some(ci) = cur {
                cur = {
                    let mut t = ci;
                    if self.store.iter_next(&mut t) {
                        Some(t)
                    } else {
                        None
                    }
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

    fn iters_for(&self, path: &str) -> Vec<gtk::TreeIter> {
        let mut found = Vec::new();
        self.walk_iters(None, path, &mut found);
        found
    }

    fn walk_iters(
        &self,
        parent: Option<&gtk::TreeIter>,
        path: &str,
        found: &mut Vec<gtk::TreeIter>,
    ) {
        let mut cur = match parent {
            None => self.store.iter_first(),
            Some(p) => self.store.iter_children(Some(p)),
        };
        while let Some(i) = cur {
            cur = {
                let mut t = i;
                if self.store.iter_next(&mut t) {
                    Some(t)
                } else {
                    None
                }
            };
            let is_dir = self.row_bool(&i, IS_DIR);
            if self.row_str(&i, PATH) == path {
                found.push(i);
            }
            if is_dir {
                self.walk_iters(Some(&i), path, found);
            }
        }
    }

    fn activated(&mut self, tpath: &gtk::TreePath, sender: &ComponentSender<Self>) {
        let Some(it) = self.store.iter(tpath) else {
            return;
        };
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
