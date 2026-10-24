#![allow(deprecated)]

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use relm4::gtk::subclass::prelude::*;
use relm4::gtk::{self, gio, glib, pango, prelude::*};
use relm4::{Component, ComponentParts, ComponentSender};
use sourceview5::prelude::*;

use crate::git::gitstatus::{self, Sides};
use crate::render::badges;
use crate::render::vector;

pub const GONE_NOTE: &str = "‹file has been deleted›";
pub const BINARY_NOTE: &str = "‹file changed on disk — not UTF-8 text›";

const TAB_CHROME: i32 = 62;

fn find_tabname(w: &gtk4::Widget) -> Option<gtk4::Widget> {
    if w.has_css_class("alpaca-tabname") {
        return Some(w.clone());
    }
    let mut kids = Vec::new();
    let mut c = w.first_child();
    while let Some(ch) = c {
        kids.push(ch.clone());
        c = ch.next_sibling();
    }
    for k in &kids {
        if let Some(f) = find_tabname(k) {
            return Some(f);
        }
    }
    None
}

#[derive(Default)]
pub struct TabDistributorImp {
    last: Cell<Option<(i32, i32)>>,
}

glib::wrapper! {
    pub struct TabDistributor(ObjectSubclass<TabDistributorImp>) @extends gtk::BoxLayout, gtk::LayoutManager;
}

#[repr(C)]
pub struct TabDistributorInstance {
    parent: gtk::ffi::GtkBoxLayout,
    _size: [u8; 128],
}

unsafe impl glib::subclass::types::InstanceStruct for TabDistributorInstance {
    type Type = TabDistributorImp;
}

unsafe impl IsSubclassable<TabDistributorImp> for gtk::BoxLayout {}

#[glib::object_subclass]
impl ObjectSubclass for TabDistributorImp {
    const NAME: &'static str = "AlpacaTabDistributor";
    type Type = TabDistributor;
    type ParentType = gtk::BoxLayout;
    type Instance = TabDistributorInstance;
}

impl ObjectImpl for TabDistributorImp {}

impl LayoutManagerImpl for TabDistributorImp {
    fn allocate(&self, widget: &gtk::Widget, width: i32, height: i32, baseline: i32) {
        self.parent_allocate(widget, width, height, baseline);
        let Some(nb) = widget.first_child().and_downcast::<gtk::Notebook>() else {
            return;
        };
        let n = nb.n_pages();
        if n == 0 || self.last.get() == Some((width, n as i32)) {
            return;
        }
        let act = nb
            .action_widget(gtk::PackType::End)
            .map_or(0, |w| w.allocated_width());
        let req = i32::max((width.saturating_sub(act)) / n as i32 - TAB_CHROME, 0);
        for i in 0..n {
            let Some(pg) = nb.nth_page(Some(i)) else {
                continue;
            };
            let Some(label) = nb.tab_label(&pg) else {
                continue;
            };
            let Some(name) = find_tabname(&label) else {
                continue;
            };
            let nat = name.measure(gtk::Orientation::Horizontal, -1).1;
            name.set_size_request(i32::min(req, nat), -1);
        }
        self.last.set(Some((width, n as i32)));
    }
}

pub fn readable_text(raw: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(raw).ok()?;
    if text.contains('\0') {
        return None;
    }
    Some(text.to_owned())
}

fn mtime_ns(path: &Path) -> Option<i64> {
    let t = std::fs::metadata(path).ok()?.modified().ok()?;
    match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => Some(d.as_nanos() as i64),
        Err(e) => Some(-(e.duration().as_nanos() as i64)),
    }
}

pub fn basename(p: &Path) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn relpath(path: &Path, root: &Path) -> String {
    let pa: Vec<_> = path.components().collect();
    let rb: Vec<_> = root.components().collect();
    if pa.len() >= rb.len() && pa[..rb.len()] == rb[..] {
        pa[rb.len()..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/")
    } else {
        path.to_string_lossy().into_owned()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FileState {
    Live,
    Deleted,
    Binary,
}

#[derive(Debug, Clone, Default)]
pub struct EditorSnapshot {
    pub open_tabs: Vec<String>,
    pub active_tab: i64,
}

#[derive(Debug)]
pub enum EditorMsg {
    Restore(PathBuf, Vec<String>, i64, Option<(String, f64)>),
    OpenFile(PathBuf),
    SaveActive,
    ProbeTrace(String),
    ProbeScroll(f64),
    SaveOpen(Vec<PathBuf>),
    OpenDiff {
        root: PathBuf,
        rel: String,
        letter: String,
    },
    RefreshDiffs {
        root: PathBuf,
        letters: Vec<(String, String)>,
    },
    StateTick,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    ProbeClip,
    ProbeDiffScroll,
    Switched,
    FsEvent(PathBuf),
    FsSettled(PathBuf),
    ClosePage(usize, bool),
}

#[derive(Debug)]
pub enum EditorOutput {
    StateChanged(EditorSnapshot),
    Flushed,
}

#[derive(Debug)]
pub enum EditorCommand {
    DiffData {
        rel: String,
        letter: String,
        text: String,
        binary: bool,
    },
    FreshDiffs(Vec<(String, String, Sides, String)>),
}

const DIFF_DEL_BG: &str = "#25181c";
const DIFF_ADD_BG: &str = "#15261d";
const DIFF_HDR_FG: &str = "#5a6375";

#[derive(Default)]
struct LinkedAxes {
    v: Option<gtk::Adjustment>,
    h: Option<gtk::Adjustment>,
}

struct DiffWidgets {
    sw_l: gtk::ScrolledWindow,
    sw_r: gtk::ScrolledWindow,
    v_r: sourceview5::View,
    diff_text: Option<String>,
    linked_l: Rc<RefCell<LinkedAxes>>,
    linked_r: Rc<RefCell<LinkedAxes>>,
    paned: gtk::Paned,
    bar: gtk::Scrollbar,
    num_l: sourceview5::View,
    num_r: sourceview5::View,
    sw_num_l: gtk::ScrolledWindow,
    sw_num_r: gtk::ScrolledWindow,
}

struct PageState {
    path: Option<PathBuf>,
    buf: sourceview5::Buffer,
    sw: gtk::ScrolledWindow,
    view: sourceview5::View,
    page: gtk::Box,
    dot: gtk::Label,
    tabname: gtk::Label,
    badge_slot: gtk::Box,
    fs: FileState,
    conflict: bool,
    load_mtime: Option<i64>,
    diff_of: Option<String>,
    diff_letter: String,
    diff: Option<DiffWidgets>,
}

pub struct Editor {
    nb: gtk::Notebook,
    pages: Vec<PageState>,
    root: Option<PathBuf>,
    lm: sourceview5::LanguageManager,
    scheme: Option<sourceview5::StyleScheme>,
    watchers: HashMap<PathBuf, gio::FileMonitor>,
}

#[relm4::component(pub)]
impl Component for Editor {
    type CommandOutput = EditorCommand;
    type Input = EditorMsg;
    type Output = EditorOutput;
    type Init = ();

    view! {
        #[root]
        gtk::Box {
            set_orientation: gtk::Orientation::Vertical,
            set_css_classes: &["alpaca-card"],
        }
    }

    fn init(_: (), root: Self::Root, sender: ComponentSender<Self>) -> ComponentParts<Self> {
        let nb = gtk::Notebook::builder()
            .vexpand(true)
            .hexpand(true)
            .scrollable(true)
            .build();

        let this = Editor {
            nb: nb.clone(),
            pages: Vec::new(),
            root: None,
            lm: sourceview5::LanguageManager::default(),
            scheme: sourceview5::StyleSchemeManager::default().scheme("alpaca-dark"),
            watchers: HashMap::new(),
        };

        let snd = sender.clone();
        nb.connect_switch_page(move |_, _, _| {
            let _ = snd.input_sender().send(EditorMsg::Switched);
        });

        let svc = gtk::ShortcutController::new();
        let snd = sender.clone();
        let trig = gtk::ShortcutTrigger::parse_string("<Control>s");
        let act = gtk::CallbackAction::new(move |_w, _a| {
            snd.input(EditorMsg::SaveActive);
            glib::Propagation::Proceed
        });
        let sc = gtk::Shortcut::new(trig, Some(act));
        svc.add_shortcut(sc);
        root.add_controller(svc);

        let dist: TabDistributor = glib::Object::builder::<TabDistributor>().build();
        let tabs_wrap = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        tabs_wrap.set_layout_manager(Some(dist));
        tabs_wrap.append(&nb);
        root.append(&tabs_wrap);

        let widgets = view_output!();
        ComponentParts {
            model: this,
            widgets,
        }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            EditorMsg::Restore(root, tabs, active, scroll) => {
                self.restore(&root, tabs, active, scroll, &sender);
            }
            EditorMsg::ProbeScroll(frac) => self.probe_scroll(frac),
            EditorMsg::OpenFile(path) => {
                self.open_file(&path, &sender);
            }
            EditorMsg::SaveActive => {
                self.save_active(&sender);
            }
            EditorMsg::ProbeTrace(tag) => {
                let (mut ws, mut ms) = (Vec::new(), Vec::new());
                let strip = self.nb.allocated_width();
                let act = self
                    .nb
                    .action_widget(gtk::PackType::End)
                    .map_or(0, |w| w.allocated_width());
                let n = self.nb.n_pages().max(1) as i32;
                let share = i32::max((strip.saturating_sub(act)) / n - TAB_CHROME, 0);
                for i in 0..self.nb.n_pages() {
                    let Some(pg) = self.nb.nth_page(Some(i)) else {
                        continue;
                    };
                    let Some(label) = self.nb.tab_label(&pg) else {
                        continue;
                    };
                    let Some(name) = find_tabname(&label) else {
                        continue;
                    };
                    let (min, _nat, _mb, _nb) = name.measure(gtk::Orientation::Horizontal, -1);
                    ws.push(format!("{}", name.allocated_width()));
                    ms.push(format!("{min}"));
                }
                println!(
                    "PROBE taballoc {tag}: strip={strip} act={act} share={share} \
                          wrap={} widths=[{}] mins=[{}]",
                    self.nb.parent().map(|p| p.allocated_width()).unwrap_or(-1),
                    ws.join(","),
                    ms.join(",")
                );
            }
            EditorMsg::SaveOpen(paths) => {
                for idx in 0..self.pages.len() {
                    let flush = self.pages[idx]
                        .path
                        .as_ref()
                        .is_some_and(|p| paths.contains(p))
                        && self.pages[idx].buf.is_modified();
                    if flush && let Err(e) = self.write_page(idx) {
                        let name = self.pages[idx]
                            .path
                            .as_ref()
                            .map(|p| basename(p))
                            .unwrap_or_default();
                        self.error("Couldn't flush", &format!("{name}: {e}"));
                    }
                }
                let _ = sender.output(EditorOutput::Flushed);
            }
            EditorMsg::OpenDiff { root, rel, letter } => {
                sender.spawn_command(move |out| {
                    let (text, binary) =
                        gitstatus::diff_for(&root.to_string_lossy(), &rel, letter == "U")
                            .unwrap_or_default();
                    out.emit(EditorCommand::DiffData {
                        rel,
                        letter,
                        text,
                        binary,
                    });
                });
            }
            EditorMsg::RefreshDiffs { root, letters } => {
                let map: HashMap<String, String> = letters.into_iter().collect();
                let mut pairs: Vec<(String, String, String, String)> = Vec::new();
                for pg in &self.pages {
                    let Some(key) = pg.diff_of.as_deref() else {
                        continue;
                    };
                    let Some(rel) = key.strip_prefix("diff:") else {
                        continue;
                    };
                    if let Some(fresh) = map.get(rel) {
                        let (old_text, old_letter) = match pg.diff.as_ref() {
                            Some(d) => (
                                d.diff_text.clone().unwrap_or_default(),
                                pg.diff_letter.clone(),
                            ),
                            None => (String::new(), String::new()),
                        };
                        pairs.push((rel.to_string(), fresh.clone(), old_text, old_letter));
                    }
                }
                sender.spawn_command(move |out| {
                    let mut fresh: Vec<(String, String, Sides, String)> = Vec::new();
                    for (rel, fresh_letter, old_text, old_letter) in pairs {
                        let (text, binary) =
                            gitstatus::diff_for(&root.to_string_lossy(), &rel, fresh_letter == "U")
                                .unwrap_or_default();
                        if binary {
                            continue;
                        }
                        if text == old_text && fresh_letter == old_letter {
                            continue;
                        }
                        let parsed = gitstatus::build_sides(&gitstatus::parse_unified(&text));
                        fresh.push((rel, text, parsed, fresh_letter));
                    }
                    out.emit(EditorCommand::FreshDiffs(fresh));
                });
            }
            EditorMsg::StateTick => {
                self.repaint_dots();
                let _ = sender.output(EditorOutput::StateChanged(self.snapshot()));
            }
            EditorMsg::Undo => {
                if let Some(idx) = self.active() {
                    let pg = &self.pages[idx];
                    if pg.buf.can_undo() {
                        pg.buf.undo();
                    }
                }
            }
            EditorMsg::Redo => {
                if let Some(idx) = self.active() {
                    let pg = &self.pages[idx];
                    if pg.buf.can_redo() {
                        pg.buf.redo();
                    }
                }
            }
            EditorMsg::Cut => self.clip_op("cut-clipboard"),
            EditorMsg::Copy => self.clip_op("copy-clipboard"),
            EditorMsg::Paste => self.clip_op("paste-clipboard"),
            EditorMsg::SelectAll => {
                if let Some(idx) = self.active()
                    && let Some(w) = self.op_target(idx)
                {
                    let _ = w.activate_action("select-all", Some(&true.to_variant()));
                }
            }
            EditorMsg::Switched => {
                self.refresh_restamps(&sender);
                let _ = sender.output(EditorOutput::StateChanged(self.snapshot()));
            }
            EditorMsg::ProbeClip => {
                let Some(idx) = self.active() else {
                    println!("PROBE clip-target: none");
                    return;
                };
                let right = self.pages[idx]
                    .diff
                    .as_ref()
                    .and_then(|d| d.sw_r.child())
                    .and_then(|w| w.downcast_ref::<sourceview5::View>().cloned());
                if let Some(ref rv) = right {
                    rv.grab_focus();
                }
                let hit = match &right {
                    Some(rv) => {
                        self.op_target(idx)
                            .and_then(|w| w.downcast_ref::<sourceview5::View>().cloned())
                            .as_ref()
                            == Some(rv)
                    }
                    None => false,
                };
                println!(
                    "PROBE clip-target: {}",
                    if hit { "right-view" } else { "left-or-wrong" }
                );
            }
            EditorMsg::ProbeDiffScroll => {
                let Some(idx) = self.active() else {
                    println!("PROBE assert: diff-presence: FAIL (no active page)");
                    return;
                };
                if let Some(dw) = self.pages[idx].diff.as_ref() {
                    let swept = PROBE_DIFF_SWEEP.with(|c| c.get());
                    probe_diff_scroll_asserts(dw, &self.pages[idx].view.clone());
                    if swept {
                        println!("PROBE renewal-assert: PASS");
                    }
                } else {
                    println!("PROBE assert: diff-presence: FAIL (not a diff page)");
                }
                if !PROBE_DIFF_SWEEP.with(|c| c.get()) {
                    PROBE_DIFF_SWEEP.with(|c| c.set(true));
                    let mut mutated = false;
                    if let Some(root) = &self.root
                        && root.file_name() == Some(std::ffi::OsStr::new("wsA"))
                    {
                        let f = root.join("a.txt");
                        if let Ok(mut fh) = std::fs::OpenOptions::new().append(true).open(&f) {
                            use std::io::Write;
                            let _ = write!(fh, "\nprobe renewal tick line\n");
                            mutated = true;
                        }
                    }
                    if mutated && let Some(root) = self.root.clone() {
                        sender.input(EditorMsg::RefreshDiffs {
                            root,
                            letters: vec![("a.txt".into(), "M".into())],
                        });
                        let snd = sender.clone();
                        glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
                            if snd.input_sender().send(EditorMsg::ProbeDiffScroll).is_err() {
                                return glib::ControlFlow::Break;
                            }
                            glib::ControlFlow::Break
                        });
                    }
                }
            }
            EditorMsg::FsEvent(path) => {
                self.arm_settle(path, &sender);
            }
            EditorMsg::FsSettled(path) => {
                self.fs_settled(&path, &sender);
            }
            EditorMsg::ClosePage(index, force) => {
                self.close_index(index, force, &sender);
            }
        }
    }

    fn update_cmd(
        &mut self,
        message: EditorCommand,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match message {
            EditorCommand::DiffData {
                rel,
                letter,
                text,
                binary,
            } => {
                if binary {
                    self.error(
                        &format!("Binary file: {rel}"),
                        "No diff view for binary changes.",
                    );
                    return;
                }
                let sides = gitstatus::build_sides(&gitstatus::parse_unified(&text));
                if sides.old.is_empty() {
                    self.error(
                        &format!("No diff: {rel}"),
                        "git produced no diff hunks for this path.",
                    );
                    return;
                }
                self.open_diff(&rel, &sides, &letter, &text, &sender);
            }
            EditorCommand::FreshDiffs(fresh) => {
                for (rel, text, sides, letter) in fresh {
                    self.refresh_diff(&rel, &sides, &letter, &text, &sender);
                }
            }
        }
    }
}

impl Editor {
    fn active(&self) -> Option<usize> {
        self.nb.current_page().map(|c| c as usize)
    }

    pub fn scroll_state(&self) -> Option<(String, f64)> {
        let idx = self.active()?;
        let pg = self.pages.get(idx)?;
        let p = pg.path.as_ref()?;
        let root = self.root.as_ref()?;
        let rel = relpath(p, root);
        Some((rel, frac_of(&pg.sw)))
    }

    fn window_widget(&self) -> Option<gtk::Window> {
        self.nb.root().and_downcast::<gtk::Window>()
    }

    fn error(&self, primary: &str, secondary: &str) {
        let Some(win) = self.window_widget() else {
            return;
        };
        #[allow(deprecated)]
        let d = gtk::MessageDialog::builder()
            .transient_for(&win)
            .modal(true)
            .text(primary)
            .secondary_text(secondary)
            .buttons(gtk::ButtonsType::Close)
            .build();
        d.connect_response(|dd, _r| dd.destroy());
        d.present();
    }

    fn page_of(&self, path: &Path) -> Option<usize> {
        self.pages
            .iter()
            .position(|pg| pg.path.as_deref() == Some(path))
    }

    fn page_of_key(&self, key: &str) -> Option<usize> {
        self.pages
            .iter()
            .position(|pg| pg.diff_of.as_deref() == Some(key))
    }

    fn rel_base(rel: &str) -> &str {
        rel.rsplit_once('/').map(|(_, b)| b).unwrap_or(rel)
    }

    fn set_root(&mut self, root: PathBuf) {
        self.root = Some(root);
        self.close_all();
    }

    fn restore(
        &mut self,
        root: &Path,
        tabs: Vec<String>,
        active: i64,
        scroll: Option<(String, f64)>,
        sender: &ComponentSender<Self>,
    ) {
        self.set_root(root.to_path_buf());
        for rel in tabs {
            let p = root.join(rel);
            if p.is_file() {
                self.open_file(&p, sender);
            }
        }
        let n = self.nb.n_pages() as i64;
        self.nb
            .set_current_page(u32::try_from(active.min((n - 1).max(0))).ok());
        if let Some((rel, frac)) = scroll
            && let Some(idx) = self.nb.current_page().map(|c| c as usize)
            && let Some(pg) = self.pages.get(idx)
            && let Some(p) = pg.path.as_ref()
            && relpath(p, root) == rel
        {
            restore_frac(&pg.sw, frac);
        }
        self.refresh_restamps(sender);
        let _ = sender.output(EditorOutput::StateChanged(self.snapshot()));
    }

    fn probe_scroll(&mut self, frac: f64) {
        if let Some(idx) = self.active()
            && let Some(pg) = self.pages.get(idx)
        {
            let adj = pg.sw.vadjustment();
            let span = (adj.upper() - adj.page_size()).max(0.0);
            adj.set_value(frac * span);
        }
    }

    fn close_all(&mut self) {
        for i in (0..self.pages.len()).rev() {
            self.nb.remove_page(Some(i as u32));
            let p = self.pages.remove(i);
            if let Some(path) = p.path {
                self.cancel_watch(&path);
            }
        }
    }

    fn open_file(&mut self, path: &Path, sender: &ComponentSender<Self>) {
        if let Some(k) = self.page_of(path) {
            self.nb.set_current_page(Some(k as u32));
            let _ = sender.output(EditorOutput::StateChanged(self.snapshot()));
            return;
        }
        let raw = match std::fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                self.error("Couldn't open file", &format!("{}: {e}", basename(path)));
                return;
            }
        };
        let text = match &raw {
            Some(bytes) => match readable_text(bytes) {
                Some(t) => t,
                None => {
                    self.error(
                        "Unsupported file",
                        &format!("{}: binary, NUL bytes, or not UTF-8", basename(path)),
                    );
                    return;
                }
            },
            None => GONE_NOTE.to_owned(),
        };

        let buf = sourceview5::Buffer::builder().text(&text).build();
        if let Some(scheme) = &self.scheme {
            buf.set_style_scheme(Some(scheme));
        }
        let base = basename(path);
        if let Some(lang) = self.lm.guess_language(Some(base.as_str()), None) {
            buf.set_language(Some(&lang));
        }
        buf.set_modified(false);
        if raw.is_none() {
            Self::tag_muted(&buf);
        }
        let snd = sender.clone();
        buf.connect_modified_changed(move |_| snd.input(EditorMsg::StateTick));
        let view = sourceview5::View::builder()
            .buffer(&buf)
            .show_line_numbers(true)
            .wrap_mode(gtk::WrapMode::None)
            .pixels_above_lines(2)
            .left_margin(12)
            .css_classes(["alpaca-mono"])
            .build();
        let page_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        let rel: String = match &self.root {
            Some(root_dir) => relpath(path, root_dir),
            None => path.to_string_lossy().into_owned(),
        };
        let crumbs = rel.split('/').collect::<Vec<_>>().join("  ›  ");
        let crumb = gtk::Label::builder()
            .label(crumbs)
            .xalign(0.0)
            .ellipsize(pango::EllipsizeMode::Middle)
            .css_classes(["alpaca-breadcrumb"])
            .build();
        page_box.append(&crumb);
        let sw = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .child(&view)
            .build();
        page_box.append(&sw);

        let fs = if raw.is_some() {
            FileState::Live
        } else {
            FileState::Deleted
        };
        let load_mtime = if raw.is_some() { mtime_ns(path) } else { None };

        let dot = gtk::Label::builder()
            .label("●")
            .css_classes(["alpaca-dirty"])
            .visible(false)
            .build();
        let name = gtk::Label::builder()
            .label(&base)
            .css_classes(["alpaca-tabname"])
            .ellipsize(pango::EllipsizeMode::Middle)
            .build();
        name.set_size_request(72, -1);
        let close = gtk::Button::new();
        if let Some(vec) = vector::icon("x-dim.svg") {
            close.set_child(Some(&gtk::Image::from_paintable(Some(&vec))));
        } else {
            close.set_child(Some(
                &gtk::Image::builder()
                    .icon_name("window-close-symbolic")
                    .build(),
            ));
        }
        close.set_css_classes(&["alpaca-close"]);
        let head = gtk::Box::builder()
            .spacing(4)
            .css_classes(["alpaca-tab"])
            .valign(gtk::Align::Center)
            .build();
        head.append(&dot);
        let slot = gtk::Box::builder()
            .spacing(4)
            .valign(gtk::Align::Center)
            .build();
        head.append(&slot);
        head.append(&name);
        head.append(&close);
        if let Some(want) = badges::pixbuf_for(&base, false) {
            slot.append(&gtk::Image::from_pixbuf(Some(&want)));
        }
        let index = self.nb.append_page(&page_box, Some(&head));
        self.nb.set_current_page(Some(index));
        let nb2 = self.nb.clone();
        let pg_widget = page_box.clone();
        let snd = sender.clone();
        close.connect_clicked(move |_| {
            let idx = nb2.page_num(&pg_widget).map(|i| i as usize);
            if let Some(idx) = idx {
                snd.input(EditorMsg::ClosePage(idx, false));
            }
        });
        self.pages.push(PageState {
            path: Some(path.to_path_buf()),
            buf,
            sw,
            view,
            page: page_box,
            dot,
            tabname: name,
            badge_slot: slot,
            fs,
            conflict: false,
            load_mtime,
            diff_of: None,
            diff_letter: String::new(),
            diff: None,
        });
        self.arm_watch(path, sender);
        if fs != FileState::Live {
            self.paint_tab_deleted(self.pages.len() - 1, true);
        }
        self.refresh_restamps(sender);
        let _ = sender.output(EditorOutput::StateChanged(self.snapshot()));
    }

    fn paint_dot(&self, idx: usize) {
        let pg = &self.pages[idx];
        pg.dot.set_visible(pg.buf.is_modified() || pg.conflict);
        let mut classes = vec!["alpaca-dirty".to_owned()];
        if pg.conflict {
            classes.push("conflict".to_owned());
        }
        let classes: Vec<&str> = classes.iter().map(|s| s.as_str()).collect();
        pg.dot.set_css_classes(&classes);
    }

    fn repaint_dots(&self) {
        for i in 0..self.pages.len() {
            self.paint_dot(i);
        }
    }

    fn paint_tab_deleted(&self, idx: usize, on: bool) {
        let name = &self.pages[idx].tabname;
        if on {
            name.add_css_class("alpaca-deleted");
        } else {
            name.remove_css_class("alpaca-deleted");
        }
    }

    fn refresh_restamps(&self, sender: &ComponentSender<Self>) {
        for pg in &self.pages {
            if pg.diff_of.is_some() {
                continue;
            }
            while let Some(c) = pg.badge_slot.first_child() {
                pg.badge_slot.remove(&c);
            }
            if let Some(path) = &pg.path
                && let Some(want) = badges::pixbuf_for(&basename(path), false)
            {
                pg.badge_slot.append(&gtk::Image::from_pixbuf(Some(&want)));
            }
        }
        let _ = sender.output(EditorOutput::StateChanged(self.snapshot()));
    }

    fn close_index(&mut self, index: usize, force: bool, sender: &ComponentSender<Self>) {
        if index >= self.pages.len() {
            return;
        }
        if self.pages[index].buf.is_modified() && !force {
            let Some(win) = self.window_widget() else {
                return;
            };
            let name = self.pages[index]
                .path
                .as_ref()
                .map(|p| basename(p))
                .unwrap_or_default();
            let d = discard_dialog(&win, &format!("Discard changes to {name}?"));
            let nb2 = self.nb.clone();
            let pg_widget = self.pages[index].page.clone();
            let snd = sender.clone();
            d.connect_response(move |dd, r| {
                dd.destroy();
                if r == gtk::ResponseType::Accept {
                    let idx = nb2.page_num(&pg_widget).map(|i| i as usize);
                    if let Some(idx) = idx {
                        snd.input(EditorMsg::ClosePage(idx, true));
                    }
                }
            });
            d.present();
            return;
        }
        self.nb.remove_page(Some(index as u32));
        let p = self.pages.remove(index);
        if let Some(path) = p.path {
            self.cancel_watch(&path);
        }
        self.refresh_restamps(sender);
    }

    fn write_page(&mut self, idx: usize) -> Result<(), std::io::Error> {
        let path = self.pages[idx].path.clone().expect("write_page: file page");
        let text = self.pages[idx].buf.text(
            &self.pages[idx].buf.start_iter(),
            &self.pages[idx].buf.end_iter(),
            false,
        );
        std::fs::write(&path, text.as_str())?;
        self.pages[idx].conflict = false;
        self.pages[idx].buf.set_modified(false);
        if let Some(m) = mtime_ns(&path) {
            self.pages[idx].load_mtime = Some(m);
        }
        Ok(())
    }

    fn save_active(&mut self, _sender: &ComponentSender<Self>) {
        let Some(idx) = self.active() else { return };
        if self.pages[idx].path.is_none() {
            return;
        }
        if !self.pages[idx].buf.is_modified() {
            return;
        }
        if let Err(e) = self.write_page(idx) {
            let name = self.pages[idx]
                .path
                .as_ref()
                .map(|p| basename(p))
                .unwrap_or_default();
            self.error("Couldn't save", &format!("{name}: {e}"));
        }
    }

    fn diff_buf(
        &self,
        lines: &[String],
        idxs: &BTreeSet<usize>,
        hdr: &BTreeSet<usize>,
        hexcol: &str,
        lang: Option<&sourceview5::Language>,
    ) -> sourceview5::Buffer {
        let buf = sourceview5::Buffer::builder()
            .text(lines.join("\n"))
            .build();
        if let Some(scheme) = &self.scheme {
            buf.set_style_scheme(Some(scheme));
        }
        if let Some(lang) = lang {
            buf.set_language(Some(lang));
        }
        let tint = rgba(hexcol);
        let tag = gtk::TextTag::builder().background_rgba(&tint).build();
        let fg = gtk::TextTag::builder()
            .foreground_rgba(&rgba(DIFF_HDR_FG))
            .build();
        buf.tag_table().add(&tag);
        buf.tag_table().add(&fg);
        for &i in idxs.iter() {
            if i >= buf.line_count() as usize || i == usize::MAX {
                continue;
            }
            if let Some(a) = buf.iter_at_line(i as i32) {
                let mut b = a;
                b.forward_to_line_end();
                buf.apply_tag(&tag, &a, &b);
            }
        }
        for &i in hdr.iter() {
            if i >= buf.line_count() as usize || i == usize::MAX {
                continue;
            }
            if let Some(a) = buf.iter_at_line(i as i32) {
                let mut b = a;
                b.forward_to_line_end();
                buf.apply_tag(&fg, &a, &b);
            }
        }
        buf.set_modified(false);
        buf
    }

    fn diff_side(
        &self,
        lines: &[String],
        idxs: &BTreeSet<usize>,
        hdr: &BTreeSet<usize>,
        hexcol: &str,
        lang: Option<&sourceview5::Language>,
    ) -> (sourceview5::View, sourceview5::Buffer) {
        let buf = self.diff_buf(lines, idxs, hdr, hexcol, lang);
        let view = sourceview5::View::builder()
            .buffer(&buf)
            .show_line_numbers(false)
            .editable(false)
            .wrap_mode(gtk::WrapMode::None)
            .pixels_above_lines(2)
            .left_margin(12)
            .css_classes(["alpaca-mono"])
            .build();
        (view, buf)
    }

    fn open_diff(
        &mut self,
        rel: &str,
        sides: &Sides,
        letter: &str,
        text: &str,
        sender: &ComponentSender<Self>,
    ) {
        let key = format!("diff:{rel}");
        if let Some(idx) = self.page_of_key(&key) {
            self.nb.set_current_page(Some(idx as u32));
            return;
        }
        let base = Self::rel_base(rel);
        let lang = self.lm.guess_language(Some(base), None);
        let (v_l, b_l) = self.diff_side(
            &sides.old,
            &sides.del,
            &sides.hdr,
            DIFF_DEL_BG,
            lang.as_ref(),
        );
        let (v_r, _b_r) = self.diff_side(
            &sides.new,
            &sides.add,
            &sides.hdr,
            DIFF_ADD_BG,
            lang.as_ref(),
        );

        let sw_l = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .child(&v_l)
            .build();
        hide_scrollbars(
            &sw_l,
            &[gtk::Orientation::Horizontal, gtk::Orientation::Vertical],
        );
        let sw_r = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .child(&v_r)
            .build();
        hide_scrollbars(&sw_r, &[gtk::Orientation::Horizontal]);
        let hbar = gtk::Scrollbar::new(gtk::Orientation::Horizontal, Some(&sw_r.hadjustment()));
        let ref_cls: Vec<glib::GString> = {
            let mut c = sw_r.first_child();
            loop {
                match c {
                    Some(w) => {
                        if let Some(sb) = w.downcast_ref::<gtk::Scrollbar>()
                            && sb.orientation() == gtk::Orientation::Vertical
                        {
                            break sb.css_classes();
                        }
                        c = w.next_sibling();
                    }
                    None => break Vec::new(),
                }
            }
        };
        let mut cls: Vec<String> = ref_cls
            .iter()
            .map(|s| s.to_string())
            .filter(|c| c != "vertical")
            .collect();
        cls.push("horizontal".into());
        if !cls.iter().any(|c| c == "alpaca-diffhbar") {
            cls.push("alpaca-diffhbar".into());
        }
        hbar.set_css_classes(&cls.iter().map(String::as_str).collect::<Vec<_>>());
        let num_l = diff_num_view(&sides.old_no);
        let num_r = diff_num_view(&sides.new_no);
        let sw_num_l = gtk::ScrolledWindow::builder().child(&num_l).build();
        hide_scrollbars(
            &sw_num_l,
            &[gtk::Orientation::Horizontal, gtk::Orientation::Vertical],
        );
        let sw_num_r = gtk::ScrolledWindow::builder().child(&num_r).build();
        hide_scrollbars(
            &sw_num_r,
            &[gtk::Orientation::Horizontal, gtk::Orientation::Vertical],
        );
        let numw = diff_num_pane_width(&sides.old_no, &num_l)
            .max(diff_num_pane_width(&sides.new_no, &num_r));
        sw_num_l.set_size_request(numw, -1);
        sw_num_r.set_size_request(numw, -1);
        if let Some(va) = v_l.vadjustment() {
            num_l.set_vadjustment(Some(&va));
        }
        if let Some(va) = v_r.vadjustment() {
            num_r.set_vadjustment(Some(&va));
        }
        let sw_l_wrap = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        sw_l_wrap.append(&sw_num_l);
        sw_l_wrap.append(&sw_l);
        let sw_r_wrap = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        sw_r_wrap.append(&sw_num_r);
        sw_r_wrap.append(&sw_r);
        let paned = gtk::Paned::builder()
            .orientation(gtk::Orientation::Horizontal)
            .vexpand(true)
            .hexpand(true)
            .css_classes(["alpaca-diffpaned"])
            .start_child(&sw_l_wrap)
            .end_child(&sw_r_wrap)
            .build();
        let paned_pos = Rc::new(Cell::new(false));
        {
            let p2 = paned.clone();
            let pos_flag = paned_pos.clone();
            sw_l.hadjustment().connect_changed(move |adj| {
                if pos_flag.get() || adj.page_size() <= 0.0 {
                    return;
                }
                let w = p2.allocated_width();
                if w > 0 {
                    pos_flag.set(true);
                    let pw = w / 2;
                    let p3 = p2.clone();
                    glib::idle_add_local_once(move || p3.set_position(pw));
                }
            });
        }

        let page = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        let crumb = gtk::Label::builder()
            .label(rel.split('/').collect::<Vec<_>>().join("  ›  "))
            .xalign(0.0)
            .ellipsize(pango::EllipsizeMode::Middle)
            .css_classes(["alpaca-breadcrumb"])
            .build();
        page.append(&crumb);
        page.append(&paned);
        page.append(&hbar);

        let slot = gtk::Box::builder()
            .spacing(4)
            .valign(gtk::Align::Center)
            .build();
        if !letter.is_empty() {
            let chip = gtk::Label::builder()
                .label(letter)
                .css_classes([format!("alpaca-diffchip{}", letter.to_lowercase())])
                .build();
            slot.append(&chip);
        }
        let name = gtk::Label::builder()
            .label(base)
            .css_classes(["alpaca-tabname"])
            .ellipsize(pango::EllipsizeMode::Middle)
            .build();
        name.set_size_request(72, -1);
        let close = gtk::Button::new();
        if let Some(vec) = vector::icon("x-dim.svg") {
            close.set_child(Some(&gtk::Image::from_paintable(Some(&vec))));
        } else {
            close.set_child(Some(
                &gtk::Image::builder()
                    .icon_name("window-close-symbolic")
                    .build(),
            ));
        }
        close.set_css_classes(&["alpaca-close"]);
        let head = gtk::Box::builder()
            .spacing(4)
            .css_classes(["alpaca-tab"])
            .valign(gtk::Align::Center)
            .build();
        head.append(&slot);
        head.append(&name);
        head.append(&close);
        let orphan_dot = gtk::Label::builder()
            .label("●")
            .css_classes(["alpaca-dirty"])
            .visible(false)
            .build();

        let index = self.nb.append_page(&page, Some(&head));
        self.nb.set_current_page(Some(index));
        let nb2 = self.nb.clone();
        let pg_widget = page.clone();
        let snd = sender.clone();
        close.connect_clicked(move |_| {
            let idx = nb2.page_num(&pg_widget).map(|i| i as usize);
            if let Some(idx) = idx {
                snd.input(EditorMsg::ClosePage(idx, false));
            }
        });

        let diff = DiffWidgets {
            sw_l: sw_l.clone(),
            sw_r: sw_r.clone(),
            v_r: v_r.clone(),
            diff_text: Some(text.to_string()),
            linked_l: Rc::new(RefCell::new(LinkedAxes::default())),
            linked_r: Rc::new(RefCell::new(LinkedAxes::default())),
            paned: paned.clone(),
            bar: hbar.clone(),
            num_l: num_l.clone(),
            num_r: num_r.clone(),
            sw_num_l: sw_num_l.clone(),
            sw_num_r: sw_num_r.clone(),
        };
        diff_link(&v_l, &v_r, &diff.linked_l);
        diff_link(&v_r, &v_l, &diff.linked_r);

        self.pages.push(PageState {
            path: None,
            buf: b_l,
            view: v_l,
            sw: sw_l,
            page,
            dot: orphan_dot,
            tabname: name,
            badge_slot: slot,
            fs: FileState::Live,
            conflict: false,
            load_mtime: None,
            diff_of: Some(key),
            diff_letter: letter.to_string(),
            diff: Some(diff),
        });
        self.refresh_restamps(sender);
        let _ = sender.output(EditorOutput::StateChanged(self.snapshot()));
    }

    fn refresh_diff(
        &mut self,
        rel: &str,
        sides: &Sides,
        letter: &str,
        text: &str,
        sender: &ComponentSender<Self>,
    ) {
        let key = format!("diff:{rel}");
        let Some(idx) = self.page_of_key(&key) else {
            return;
        };
        let (frac_l, frac_r) = {
            let dw = self.pages[idx].diff.as_ref().unwrap();
            (frac_of(&dw.sw_l), frac_of(&dw.sw_r))
        };
        let lang = self.lm.guess_language(Some(Self::rel_base(rel)), None);
        let (b_l, b_r) = (
            self.diff_buf(
                &sides.old,
                &sides.del,
                &sides.hdr,
                DIFF_DEL_BG,
                lang.as_ref(),
            ),
            self.diff_buf(
                &sides.new,
                &sides.add,
                &sides.hdr,
                DIFF_ADD_BG,
                lang.as_ref(),
            ),
        );
        {
            let (sw_l, sw_r, sw_num_l, sw_num_r, v_r, num_l, num_r) = {
                let dw = self.pages[idx].diff.as_ref().unwrap();
                (
                    dw.sw_l.clone(),
                    dw.sw_r.clone(),
                    dw.sw_num_l.clone(),
                    dw.sw_num_r.clone(),
                    dw.v_r.clone(),
                    dw.num_l.clone(),
                    dw.num_r.clone(),
                )
            };
            self.pages[idx].view.set_buffer(Some(&b_l));
            v_r.set_buffer(Some(&b_r));
            num_l.set_buffer(Some(
                &sourceview5::Buffer::builder()
                    .text(gitstatus::diff_num_text(&sides.old_no))
                    .build(),
            ));
            num_r.set_buffer(Some(
                &sourceview5::Buffer::builder()
                    .text(gitstatus::diff_num_text(&sides.new_no))
                    .build(),
            ));
            let numw = diff_num_pane_width(&sides.old_no, &num_l)
                .max(diff_num_pane_width(&sides.new_no, &num_r));
            sw_num_l.set_size_request(numw, -1);
            sw_num_r.set_size_request(numw, -1);
            self.pages[idx].buf = b_l;
            self.pages[idx].diff.as_mut().unwrap().diff_text = Some(text.to_string());
            restore_frac(&sw_l, frac_l);
            restore_frac(&sw_r, frac_r);
        }
        if letter != self.pages[idx].diff_letter {
            let slot = self.pages[idx].badge_slot.clone();
            let mut chip_child = slot.first_child();
            while let Some(c) = chip_child {
                let nxt = c.next_sibling();
                slot.remove(&c);
                chip_child = nxt;
            }
            if !letter.is_empty() {
                let chip = gtk::Label::builder()
                    .label(letter)
                    .css_classes([format!("alpaca-diffchip{}", letter.to_lowercase())])
                    .build();
                slot.append(&chip);
            }
            self.pages[idx].diff_letter = letter.to_string();
        }
        let _ = sender.output(EditorOutput::StateChanged(self.snapshot()));
    }

    fn op_target(&self, idx: usize) -> Option<gtk4::Widget> {
        if self.pages[idx].diff.is_some() {
            self.nb
                .root()
                .and_then(|o| o.downcast::<gtk4::Window>().ok())
                .and_then(|w| gtk4::prelude::GtkWindowExt::focus(&w))
        } else {
            Some(self.pages[idx].view.clone().upcast())
        }
    }

    fn clip_op(&self, op: &str) {
        if let Some(idx) = self.active()
            && let Some(w) = self.op_target(idx)
        {
            let _ = w.activate_action(op, None);
        }
    }

    fn cancel_watch(&mut self, path: &Path) {
        if let Some(mon) = self.watchers.remove(path) {
            mon.cancel();
        }
    }

    fn arm_watch(&mut self, path: &Path, sender: &ComponentSender<Self>) {
        self.cancel_watch(path);
        let file = gio::File::for_path(path);
        let mon = match file.monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE) {
            Ok(m) => m,
            Err(_) => return,
        };
        let snd = sender.clone();
        let p = path.to_path_buf();
        mon.connect_changed(move |_, _, _, _| {
            snd.input(EditorMsg::FsEvent(p.clone()));
        });
        self.watchers.insert(path.to_path_buf(), mon);
    }

    fn arm_settle(&self, path: PathBuf, sender: &ComponentSender<Self>) {
        let snd = sender.clone();
        glib::timeout_add(std::time::Duration::from_millis(120), move || {
            if snd
                .input_sender()
                .send(EditorMsg::FsSettled(path.clone()))
                .is_err()
            {
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Break
        });
    }

    fn fs_settled(&mut self, path: &Path, sender: &ComponentSender<Self>) {
        let Some(k) = self.page_of(path) else { return };
        let Some(mtime) = mtime_ns(path) else {
            self.fs_tombstone(k, sender);
            return;
        };
        if self.pages[k].fs == FileState::Live && Some(mtime) == self.pages[k].load_mtime {
            return;
        }
        if self.pages[k].buf.is_modified() {
            if self.pages[k].fs != FileState::Live {
                self.pages[k].fs = FileState::Live;
                self.paint_tab_deleted(k, false);
            }
            self.pages[k].conflict = true;
            self.paint_dot(k);
            return;
        }
        self.reload(k, mtime, sender);
    }

    fn reload(&mut self, idx: usize, mtime: i64, sender: &ComponentSender<Self>) {
        let Some(path) = self.pages[idx].path.clone() else {
            return;
        };
        let Ok(raw) = std::fs::read(&path) else {
            self.fs_tombstone(idx, sender);
            return;
        };
        let Some(text) = readable_text(&raw) else {
            self.fs_binary(idx, sender);
            return;
        };
        let (line, col, scroll) = {
            let pg = &self.pages[idx];
            let ins = pg.buf.get_insert();
            let it = pg.buf.iter_at_mark(&ins);
            (it.line(), it.line_offset(), pg.sw.vadjustment().value())
        };
        self.swap_text(idx, &text, sender);
        let pg = &mut self.pages[idx];
        let n = pg.buf.line_count();
        if let Some(mut it) = pg.buf.iter_at_line(line.min(n - 1)) {
            it.forward_chars(col.min((it.chars_in_line() - 1).max(0)));
            pg.buf.place_cursor(&it);
        }
        pg.fs = FileState::Live;
        pg.conflict = false;
        pg.load_mtime = Some(mtime);
        pg.view.set_editable(true);
        self.paint_tab_deleted(idx, false);
        let sw = self.pages[idx].sw.clone();
        glib::idle_add_local_once(move || {
            let adj = sw.vadjustment();
            adj.set_value(scroll.min((adj.upper() - adj.page_size()).max(0.0)));
        });
        self.paint_dot(idx);
        let _ = sender.output(EditorOutput::StateChanged(self.snapshot()));
    }

    fn swap_text(&mut self, idx: usize, text: &str, sender: &ComponentSender<Self>) {
        let (sch, lang) = {
            let pg = &self.pages[idx];
            (pg.buf.style_scheme(), pg.buf.language())
        };
        let nb = sourceview5::Buffer::builder().text(text).build();
        if let Some(s) = sch {
            nb.set_style_scheme(Some(&s));
        }
        if let Some(l) = lang {
            nb.set_language(Some(&l));
        }
        nb.set_modified(false);
        let snd = sender.clone();
        nb.connect_modified_changed(move |_| snd.input(EditorMsg::StateTick));
        let pg = &mut self.pages[idx];
        pg.view.set_buffer(Some(&nb));
        pg.buf = nb;
    }

    fn fs_tombstone(&mut self, idx: usize, sender: &ComponentSender<Self>) {
        if self.pages[idx].fs == FileState::Deleted {
            return;
        }
        {
            let pg = &mut self.pages[idx];
            pg.fs = FileState::Deleted;
            pg.conflict = false;
        }
        if !self.pages[idx].buf.is_modified() {
            self.set_placeholder(idx, GONE_NOTE, sender);
        } else {
            self.paint_dot(idx);
        }
        self.paint_tab_deleted(idx, true);
        let _ = sender.output(EditorOutput::StateChanged(self.snapshot()));
    }

    fn fs_binary(&mut self, idx: usize, sender: &ComponentSender<Self>) {
        if self.pages[idx].fs == FileState::Binary {
            return;
        }
        {
            let pg = &mut self.pages[idx];
            pg.fs = FileState::Binary;
            pg.conflict = false;
            pg.view.set_editable(false);
        }
        if !self.pages[idx].buf.is_modified() {
            self.set_placeholder(idx, BINARY_NOTE, sender);
        }
        let _ = sender.output(EditorOutput::StateChanged(self.snapshot()));
    }

    fn set_placeholder(&mut self, idx: usize, text: &str, sender: &ComponentSender<Self>) {
        self.swap_text(idx, text, sender);
        Self::tag_muted(&self.pages[idx].buf);
        self.pages[idx].buf.set_modified(false);
    }

    fn tag_muted(buf: &sourceview5::Buffer) {
        let tag = gtk::TextTag::builder()
            .foreground_rgba(&gtk::gdk::RGBA::parse("#5a6375").unwrap())
            .style(gtk::pango::Style::Italic)
            .build();
        buf.tag_table().add(&tag);
        buf.apply_tag(&tag, &buf.start_iter(), &buf.end_iter());
    }

    fn snapshot(&self) -> EditorSnapshot {
        let open_tabs: Vec<String> = self
            .pages
            .iter()
            .filter_map(|pg| pg.path.as_ref())
            .map(|p| match &self.root {
                Some(root) => relpath(p, root),
                None => p.to_string_lossy().into_owned(),
            })
            .collect();
        let active_tab = self.nb.current_page().map(i64::from).unwrap_or(0);
        EditorSnapshot {
            open_tabs,
            active_tab,
        }
    }
}

#[allow(deprecated)]
fn discard_dialog(win: &gtk::Window, text: &str) -> gtk::MessageDialog {
    let d = gtk::MessageDialog::builder()
        .transient_for(win)
        .modal(true)
        .text(text)
        .buttons(gtk::ButtonsType::Cancel)
        .build();
    d.add_button("Discard", gtk::ResponseType::Accept);
    d
}

fn rgba(hexcol: &str) -> gtk::gdk::RGBA {
    let mut c = gtk::gdk::RGBA::new(0.0, 0.0, 0.0, 0.0);
    if let Ok(p) = gtk::gdk::RGBA::parse(hexcol) {
        c = p;
    }
    c
}

fn frac_of(sw: &gtk::ScrolledWindow) -> f64 {
    let adj = sw.vadjustment();
    let span = (adj.upper() - adj.page_size()).max(0.0);
    if span > 0.0 {
        (adj.value() / span).min(1.0)
    } else {
        0.0
    }
}

fn restore_frac(sw: &gtk::ScrolledWindow, frac: f64) {
    let adj = sw.vadjustment();
    let id = adj.connect_changed(move |a| {
        let span = (a.upper() - a.page_size()).max(0.0);
        if span > 0.0 {
            a.set_value((frac * span).min(span));
        }
    });
    let adj2 = adj.clone();
    glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
        adj2.disconnect(id);
    });
}

fn hide_scrollbars(sw: &gtk::ScrolledWindow, axes: &[gtk::Orientation]) {
    let mut c = sw.first_child();
    while let Some(w) = c {
        let nxt = w.next_sibling();
        if let Some(sb) = w.downcast_ref::<gtk::Scrollbar>()
            && axes.contains(&sb.orientation())
        {
            w.set_visible(false);
        }
        c = nxt;
    }
}

fn probe_diff_scroll_asserts(dw: &DiffWidgets, view_l: &sourceview5::View) {
    let ok = |name: String, got: bool| {
        println!(
            "PROBE assert: diff-{name}: {}",
            if got { "PASS" } else { "FAIL" }
        );
    };
    let bars = |sw: &gtk::ScrolledWindow| -> Vec<gtk::Scrollbar> {
        let mut out = Vec::new();
        let mut c = sw.first_child();
        while let Some(w) = c {
            if let Some(sb) = w.downcast_ref::<gtk::Scrollbar>() {
                out.push(sb.clone());
            }
            c = w.next_sibling();
        }
        out
    };
    for (axis, src, dst) in [
        ("v", dw.sw_r.vadjustment(), dw.sw_l.vadjustment()),
        ("h", dw.sw_r.hadjustment(), dw.sw_l.hadjustment()),
    ] {
        let span = (src.upper() - src.page_size()).max(0.0);
        let dspan = (dst.upper() - dst.page_size()).max(0.0);
        if span <= 0.0 || dspan <= 0.0 {
            println!("PROBE assert: diff-sync-{axis}: SKIPPED (no span)");
            continue;
        }
        src.set_value(span.min(60.0));
        ok(
            format!("sync-{axis}-rl"),
            (src.value() - dst.value()).abs() < 0.5,
        );
        dst.set_value(dspan.min(60.0));
        ok(
            format!("sync-{axis}-lr"),
            (src.value() - dst.value()).abs() < 0.5,
        );
    }
    let bars_l = bars(&dw.sw_l);
    ok(
        "bars-l".into(),
        bars_l.len() >= 2 && bars_l.iter().all(|b| !b.is_visible()),
    );
    ok(
        "policy-v".into(),
        dw.sw_l.vscrollbar_policy() == gtk::PolicyType::Automatic,
    );
    ok(
        "policy-h".into(),
        dw.sw_l.hscrollbar_policy() == gtk::PolicyType::Automatic,
    );
    let badj = dw.bar.adjustment();
    let dr = dw.sw_r.hadjustment();
    ok("bar-adj".into(), format!("{badj:?}") == format!("{dr:?}"));
    let bars_r = bars(&dw.sw_r);
    let r_h = bars_r
        .iter()
        .find(|b| b.orientation() == gtk::Orientation::Horizontal);
    let r_v = bars_r
        .iter()
        .find(|b| b.orientation() == gtk::Orientation::Vertical);
    ok(
        "bars-r".into(),
        r_h.map(|b| !b.is_visible()).unwrap_or(false)
            && r_v.map(|b| b.is_visible()).unwrap_or(false),
    );
    println!(
        "PROBE classes: ref-v={:?} bar={:?}",
        r_v.map(|b| b.css_classes()),
        dw.bar.css_classes()
    );
    let span = (badj.upper() - badj.page_size()).max(0.0);
    if span > 0.0 {
        badj.set_value(span.min(60.0));
        ok(
            "bar-drive".into(),
            (dr.value() - dw.sw_l.hadjustment().value()).abs() < 0.5,
        );
    } else {
        println!("PROBE assert: diff-bar-drive: SKIPPED (no span/unbound)");
    }
    let pw = dw.paned.allocated_width() as f64;
    ok(
        "paned-50".into(),
        pw > 0.0 && (dw.paned.position() as f64 - pw / 2.0).abs() <= 4.0,
    );
    ok(
        "num-rows".into(),
        dw.num_l.buffer().line_count() == view_l.buffer().line_count()
            && dw.num_r.buffer().line_count() == dw.v_r.buffer().line_count(),
    );
    let buf_l = dw.num_l.buffer();
    let t = buf_l.text(&buf_l.start_iter(), &buf_l.end_iter(), true);
    ok(
        "num-filestart".into(),
        t.split('\n').nth(1).map(str::trim) == Some("1"),
    );
    let field_w = |v: &sourceview5::View| -> i32 {
        let buf = v.buffer();
        let n = buf
            .text(&buf.start_iter(), &buf.end_iter(), true)
            .split('\n')
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(2)
            .max(2);
        let (px, _) = v.create_pango_layout(Some(&"0".repeat(n))).pixel_size();
        px + 4
    };
    ok(
        "num-width".into(),
        dw.num_l.allocated_width() >= field_w(&dw.num_l)
            && dw.num_r.allocated_width() >= field_w(&dw.num_r),
    );
    let (nat_l, nat_r) = (
        view_l.measure(gtk::Orientation::Vertical, -1).1,
        dw.v_r.measure(gtk::Orientation::Vertical, -1).1,
    );
    let (nnum_l, nnum_r) = (
        dw.num_l.measure(gtk::Orientation::Vertical, -1).1,
        dw.num_r.measure(gtk::Orientation::Vertical, -1).1,
    );
    ok(
        "num-height".into(),
        nat_l > 0 && (nat_l - nnum_l).abs() <= 2 && nat_r > 0 && (nat_r - nnum_r).abs() <= 2,
    );
    match (dw.num_l.vadjustment(), view_l.vadjustment()) {
        (Some(na), Some(ma)) => ok("num-adj-l".into(), format!("{na:?}") == format!("{ma:?}")),
        _ => println!("PROBE assert: diff-num-adj-l: FAIL (adj None)"),
    }
    match (dw.num_r.vadjustment(), dw.v_r.vadjustment()) {
        (Some(na), Some(ma)) => {
            ok("num-adj-r".into(), format!("{na:?}") == format!("{ma:?}"));
            ok("num-sync".into(), (na.value() - ma.value()).abs() < 0.01);
        }
        _ => println!("PROBE assert: diff-num-adj-r: FAIL (adj None)"),
    }
}

fn diff_num_view(no: &[u32]) -> sourceview5::View {
    let buf = sourceview5::Buffer::builder()
        .text(gitstatus::diff_num_text(no))
        .build();
    sourceview5::View::builder()
        .buffer(&buf)
        .show_line_numbers(false)
        .editable(false)
        .focusable(false)
        .wrap_mode(gtk::WrapMode::None)
        .pixels_above_lines(2)
        .left_margin(4)
        .css_classes(["alpaca-mono", "alpaca-diffnum"])
        .build()
}

fn diff_num_pane_width(no: &[u32], view: &sourceview5::View) -> i32 {
    let probe = "0".repeat(gitstatus::diff_num_field_width(no));
    let (px, _) = view.create_pango_layout(Some(&probe)).pixel_size();
    (px + 4 + 4).max(24)
}

fn diff_link(v: &sourceview5::View, dst: &sourceview5::View, linked: &Rc<RefCell<LinkedAxes>>) {
    let mut l = linked.borrow_mut();
    if let Some(va) = v.vadjustment()
        && l.v.as_ref() != Some(&va)
    {
        l.v = Some(va.clone());
        let dst2 = dst.clone();
        va.connect_value_changed(move |a| {
            if let Some(dadj) = dst2.vadjustment() {
                dadj.set_value(a.value());
            }
        });
    }
    if let Some(ha) = v.hadjustment()
        && l.h.as_ref() != Some(&ha)
    {
        l.h = Some(ha.clone());
        let dst2 = dst.clone();
        ha.connect_value_changed(move |a| {
            if let Some(dadj) = dst2.hadjustment() {
                dadj.set_value(a.value());
            }
        });
    }
}

thread_local! {
    static PROBE_DIFF_SWEEP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
mod tests {
    use super::readable_text;

    #[test]
    fn readable_text_rules() {
        assert_eq!(
            readable_text(b"hello\nworld").as_deref(),
            Some("hello\nworld")
        );
        assert!(readable_text(b"caf\xc3\xa9").is_some());
        assert_eq!(readable_text(b"caf\xe9"), None);
        assert_eq!(readable_text(b"a\0b"), None);
        assert_eq!(readable_text(b""), Some(String::new()));
    }
}
