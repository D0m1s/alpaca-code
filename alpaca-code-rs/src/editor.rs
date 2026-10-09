//! Editor component — file tabs: open/close/save/restore/watches/tombstone/
//! binary/dirty dots/badges (python `alpaca_code/editor.py`, file-page S1 port;
//! diff pages + `_TabDistributor` is S2/S3).
//!
//! Rust keeps a parallel page model (`PageState`, index-aligned with the
//! notebook) instead of dangling attributes on the page GtkWidget — the
//! wrapper-identity trap CLAUDE.md warns about is avoided by construction, and
//! the dirty dot / tabname labels are held directly (python walks
//! `get_first_child()` to find them).

// 4.10-deprecated dialog API (the VTE open-with dialog) — same patterns as the
// Gtk4-Python source; S3 polish modernizes.
#![allow(deprecated)]

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use relm4::gtk::{self, gio, glib, pango, prelude::*};
use relm4::gtk::subclass::prelude::*;
use relm4::{Component, ComponentParts, ComponentSender};
use sourceview5::prelude::*; // BufferExt: set_style_scheme/set_language

use crate::vector;
use crate::badges;
use crate::gitstatus::{self, Sides};

pub const GONE_NOTE: &str = "‹file has been deleted›";
pub const BINARY_NOTE: &str = "‹file changed on disk — not UTF-8 text›";

/// chrome subtracted before sharing a tab's width: badge slot + ✕ + label/
/// head paddings (@13px measured on python; starting hypothesis from python's
/// code value 62 — re-measure live if `max(wide) > share` ever trips)
const TAB_CHROME: i32 = 62;

/// walk a widget tree hunting a descendant carrying the tab-name class
/// (used by both the distributor's tab walk and ProbeTrace)
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

/// `_TabDistributor` port (editor.py:12-49): the layout manager of the
/// vertical wrapper Box around the notebook strip. GTK 4.22 pins scrollable
/// notebook tabs at their minimum width even with room to spare, and scroll
/// mode's visible tail grows WIDER as the window narrows — this layout
/// manager instead floors every tab-name label at
/// min(share, natural): req = (wrap_width − end_action_w) // n − CHROME.
/// Recomputes only on (width, n) change.
#[derive(Default)]
pub struct TabDistributorImp {
    last: Cell<Option<(i32, i32)>>, // (wrap width, nb page count) — python's _last
}

glib::wrapper! {
    pub struct TabDistributor(ObjectSubclass<TabDistributorImp>) @extends gtk::BoxLayout, gtk::LayoutManager;
}

/// TabDistributor's C instance struct. glib's basic InstanceStruct embeds
/// `<TabDistributor as ObjectType>::GlibType` = ffi::GtkBoxLayout, which gir
/// emitted OPAQUE (0 bytes — GtkBoxLayout exposes no public fields), so
/// registration computed instance size 0 and GObject rejected the type
/// ("instance size smaller than GTypeInstance size"). Pad the C layout to the
/// real GtkBoxLayout instance footprint (GObject + LayoutManager + BoxLayout
/// privates — ~84B on this build, well under 128): the imp's PrivateStruct is
/// appended AFTER the instance struct by g_type_add_instance_private.
#[repr(C)]
pub struct TabDistributorInstance {
    parent: gtk::ffi::GtkBoxLayout,
    _size: [u8; 128],
}

unsafe impl glib::subclass::types::InstanceStruct for TabDistributorInstance {
    type Type = TabDistributorImp;
}

// gtk4 0.11.5 registers IsSubclassable for LayoutManager but generates none
// for BoxLayout (it has no overridable vfuncs, so gir skipped the link) —
// the subclass chain between ParentType=BoxLayout and LayoutManager needs it.
// Concrete T (not generic): the orphan rule only allows implementing the foreign
// trait for a foreign type when a LOCAL type appears in the impl.
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
        let Some(nb) = widget.first_child().and_downcast::<gtk::Notebook>() else { return };
        let n = nb.n_pages();
        if n == 0 || self.last.get() == Some((width, n as i32)) {
            return;
        }
        // python: act_w = end-action widget's allocated width; never set in
        // this app → no widget → 0. (kept None-safe for parity)
        let act = nb
            .action_widget(gtk::PackType::End)
            .map_or(0, |w| w.allocated_width());
        let req = i32::max((width.saturating_sub(act)) / n as i32 - TAB_CHROME, 0);
        for i in 0..n {
            let Some(pg) = nb.nth_page(Some(i)) else { continue };
            let Some(label) = nb.tab_label(&pg) else { continue };
            let Some(name) = find_tabname(&label) else { continue };
            let nat = name.measure(gtk::Orientation::Horizontal, -1).1;
            name.set_size_request(i32::min(req, nat), -1);
        }
        self.last.set(Some((width, n as i32)));
    }
}

/// editor.py:readable_text — decode UTF-8; reject NUL bytes (a lossy buffer
/// plus Ctrl+S would rewrite the file with truncated/replacement garbage).
pub fn readable_text(raw: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(raw).ok()?;
    if text.contains('\0') {
        return None;
    }
    Some(text.to_owned())
}

/// os.stat().st_mtime_ns via std (negative for pre-epoch mtimes).
fn mtime_ns(path: &Path) -> Option<i64> {
    let t = std::fs::metadata(path).ok()?.modified().ok()?;
    match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => Some(d.as_nanos() as i64),
        Err(e) => Some(-(e.duration().as_nanos() as i64)),
    }
}

// python os.path.basename — the last component (empty for paths ending in /).
pub fn basename(p: &Path) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// os.path.relpath(path, root). Tabs always live under the open root (opened
/// from the tree, restored by joining), so this covers only the under-root
/// shape; an outside-root path keeps its absolute spelling.
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
    /// file gone on disk: clean buffer → tombstone note, dirty keeps user text
    Deleted,
    /// disk content unreadable: view frozen (edit-off), note placed
    Binary,
}

#[derive(Debug, Clone, Default)]
pub struct EditorSnapshot {
    pub open_tabs: Vec<String>,
    pub active_tab: i64,
    pub has_dirty: bool,
}

#[derive(Debug)]
pub enum EditorMsg {
    Restore(PathBuf, Vec<String>, i64),   // set_root + open still-existing tabs (window.py:192 is the only set path)
    OpenFile(PathBuf),
    SaveActive,
    /// PROBE-ONLY (ALPACA_PROBE_S3) — tab allocation trace (taballoc line);
    /// production never sends it (no S3 chain)
    ProbeTrace(String),
    /// commit flow flushes dirty buffers among the named abs paths
    SaveOpen(Vec<PathBuf>),
    /// CHANGES row click → side-by-side diff page (python window._open_changes_diff)
    OpenDiff {
        root: PathBuf,
        rel: String,
        letter: String,
    },
    /// fresh git data → refetch sides for every open diff page still present
    /// in the change set (python window._on_git_changed); rel→fresh-letter rows
    RefreshDiffs {
        root: PathBuf,
        letters: Vec<(String, String)>,
    },
    /// repaint every dirty dot + emit the snapshot (modified-changed, open…)
    StateTick,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    /// probe-only: exercise the Edit-menu op target on the active diff page
    /// and report which side it lands on (python _edit_op's focus route);
    /// production never sends it
    ProbeClip,
    // internal
    /// post-emission re-stamp: fires after switch-page lands, when current_page()
    /// is already the new index (the staleness window is inside the emission only)
    Switched,
    /// a file monitor fired (python `_on_file_event`)
    FsEvent(PathBuf),
    /// the 120ms settle elapsed (python `_fs_settled`)
    FsSettled(PathBuf),
    ClosePage(usize, bool),
}

#[derive(Debug)]
pub enum EditorOutput {
    StateChanged(EditorSnapshot),
    /// ack of the SaveOpen flush — ALWAYS fires, even with zero flushed pages
    /// (drives T7's pending-commit chain; a missing ack would strand the
    /// eager-busy buttons)
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
    FreshDiffs(Vec<(String, Sides, String)>),
}

// editor.py:209-211
const DIFF_DEL_BG: &str = "#25181c";
const DIFF_ADD_BG: &str = "#15261d";
const DIFF_HDR_FG: &str = "#5a6375";

/// both diff sides' scroll pairing state: the widgets the live renewal
/// re-children, plus each view's linked-adjustment dedupe (python `_linked`
/// attrs — one per direction)
struct DiffWidgets {
    sw_l: gtk::ScrolledWindow,
    sw_r: gtk::ScrolledWindow,
    linked_l: Rc<RefCell<Option<gtk::Adjustment>>>,
    linked_r: Rc<RefCell<Option<gtk::Adjustment>>>,
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
    load_mtime: Option<i64>, // st_mtime_ns
    // diff page fields (S2) — path stays None so save/reload/fs paths guard out
    diff_of: Option<String>,   // "diff:<rel>" key
    diff_letter: String,
    diff: Option<DiffWidgets>,
}

pub struct Editor {
    nb: gtk::Notebook,
    pages: Vec<PageState>,
    /// workspace root for relpaths (crumb + snapshot); None until SetRoot
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
            snd.input(EditorMsg::Switched);
        });

        // editor.py:95-104 — Ctrl+S saves the active page
        let svc = gtk::ShortcutController::new();
        let snd = sender.clone();
        let trig = gtk::ShortcutTrigger::parse_string("<Control>s");
        let act = gtk::CallbackAction::new(move |_w, _a| {
            snd.input(EditorMsg::SaveActive);
            glib::Propagation::Proceed // python's handler returns True (propagate)
        });
        let sc = gtk::Shortcut::new(trig, Some(act));
        svc.add_shortcut(sc);
        root.add_controller(svc);

        let dist: TabDistributor = glib::Object::builder::<TabDistributor>().build();
        let tabs_wrap = gtk::Box::builder().orientation(gtk::Orientation::Vertical).build();
        // python's comment, carried: the wrap must take the full row width — a
        // HORIZONTAL Box gives a non-hexpand child only its own minimum (measured
        // 900px wrap → 280px nb); VERTICAL makes x the cross axis where children FILL
        tabs_wrap.set_layout_manager(Some(dist)); // widget refs the object; local keeps the borrow
        tabs_wrap.append(&nb);
        root.append(&tabs_wrap);

        let widgets = view_output!();
        ComponentParts { model: this, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match msg {
            EditorMsg::Restore(root, tabs, active) => {
                self.restore(&root, tabs, active, &sender);
            }
            EditorMsg::OpenFile(path) => {
                self.open_file(&path, &sender);
            }
            EditorMsg::SaveActive => {
                self.save_active(&sender);
            }
            EditorMsg::ProbeTrace(tag) => {
                let (mut ws, mut ms) = (Vec::new(), Vec::new());
                let strip = self.nb.allocated_width();
                let act = self.nb.action_widget(gtk::PackType::End).map_or(0, |w| w.allocated_width());
                let n = self.nb.n_pages().max(1) as i32;
                let share = i32::max((strip.saturating_sub(act)) / n - TAB_CHROME, 0);
                for i in 0..self.nb.n_pages() {
                    let Some(pg) = self.nb.nth_page(Some(i)) else { continue };
                    let Some(label) = self.nb.tab_label(&pg) else { continue };
                    let Some(name) = find_tabname(&label) else { continue };
                    let (min, _nat, _mb, _nb) = name.measure(gtk::Orientation::Horizontal, -1);
                    ws.push(format!("{}", name.allocated_width()));
                    ms.push(format!("{min}"));
                }
                println!("PROBE taballoc {tag}: strip={strip} act={act} share={share} \
                          wrap={} widths=[{}] mins=[{}]",
                    self.nb.parent().map(|p| p.allocated_width()).unwrap_or(-1),
                    ws.join(","), ms.join(","));
            }
            EditorMsg::SaveOpen(paths) => {
                // commit flow flush: dirty buffers among the named abs paths go
                // to disk BEFORE the commit runs. Write errors are swallowed
                // and Flushed ALWAYS fires — python left the OSError unguarded
                // in its callback chain; rust must not strand T7's
                // pending-commit chain on an I/O error (eager-busy gate).
                for idx in 0..self.pages.len() {
                    let flush = self.pages[idx]
                        .path
                        .as_ref()
                        .map_or(false, |p| paths.contains(p))
                        && self.pages[idx].buf.is_modified();
                    if flush {
                        let _ = self.write_page(idx);
                    }
                }
                let _ = sender.output(EditorOutput::Flushed);
            }
            EditorMsg::OpenDiff { root, rel, letter } => {
                // root snapshot rides into the flight (App passes its live
                // root; no guard beyond snapshot — python guarded its own
                // root before dispatch)
                sender.spawn_command(move |out| {
                    let (text, binary) = gitstatus::diff_for(
                        &root.to_string_lossy(),
                        &rel,
                        letter == "U",
                    )
                    // git failed/timeout → "" → the same no-diff gate
                    .unwrap_or_default();
                    out.emit(EditorCommand::DiffData { rel, letter, text, binary });
                });
            }
            EditorMsg::RefreshDiffs { root, letters } => {
                // per OPEN diff page still present in the change set; resolved
                // tabs stay as-is. python ran this fetch SYNC on the UI
                // thread — the flight is a responsiveness improvement only,
                // identical behavior when it lands
                let map: HashMap<String, String> = letters.into_iter().collect();
                let mut pairs: Vec<(String, String)> = Vec::new();
                for pg in &self.pages {
                    let Some(key) = pg.diff_of.as_deref() else { continue };
                    let Some(rel) = key.strip_prefix("diff:") else { continue };
                    if let Some(fresh) = map.get(rel) {
                        pairs.push((rel.to_string(), fresh.clone()));
                    }
                }
                sender.spawn_command(move |out| {
                    let mut fresh: Vec<(String, Sides, String)> = Vec::new();
                    for (rel, fresh_letter) in pairs {
                        let (text, binary) = gitstatus::diff_for(
                            &root.to_string_lossy(),
                            &rel,
                            fresh_letter == "U",
                        )
                        .unwrap_or_default();
                        if binary {
                            continue; // the tab stays stale rather than die
                        }
                        fresh.push((
                            rel,
                            gitstatus::build_sides(&gitstatus::parse_unified(&text)),
                            fresh_letter,
                        ));
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
            // window.py:_edit_op — clipboard ops ride the view's own actions
            EditorMsg::Cut => self.clip_op("cut-clipboard"),
            EditorMsg::Copy => self.clip_op("copy-clipboard"),
            EditorMsg::Paste => self.clip_op("paste-clipboard"),
            EditorMsg::SelectAll => {
                if let Some(idx) = self.active() {
                    if let Some(w) = self.op_target(idx) {
                        let _ = w.activate_action("select-all", Some(&true.to_variant()));
                    }
                }
            }
            EditorMsg::Switched => {
                self.refresh_restamps(&sender);
                let _ = sender.output(EditorOutput::StateChanged(self.snapshot()));
            }
            // probe-only fix-3 instrument: resolve the SAME widget the
            // production Edit-menu route targets on a diff page, and report
            // whether it is the focused (right) side view
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
                    rv.grab_focus(); // the user's pointer/focus is on this side
                }
                // the production op route (op_target after fix-3): must land
                // on the focused right view now
                let hit = match &right {
                    Some(rv) => self
                        .op_target(idx)
                        .and_then(|w| w.downcast_ref::<sourceview5::View>().cloned())
                        .as_ref()
                        == Some(rv),
                    None => false,
                };
                println!(
                    "PROBE clip-target: {}",
                    if hit { "right-view" } else { "left-or-wrong" }
                );
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
            EditorCommand::DiffData { rel, letter, text, binary } => {
                if binary {
                    self.error(
                        &format!("Binary file: {rel}"),
                        "No diff view for binary changes.",
                    );
                    return;
                }
                let sides = gitstatus::build_sides(&gitstatus::parse_unified(&text));
                // python: `len(sides[0]) == 0` — sides[0] is the OLD side
                if sides.old.is_empty() {
                    self.error(
                        &format!("No diff: {rel}"),
                        "git produced no diff hunks for this path.",
                    );
                    return;
                }
                self.open_diff(&rel, &sides, &letter, &sender);
            }
            EditorCommand::FreshDiffs(fresh) => {
                for (rel, sides, letter) in fresh {
                    self.refresh_diff(&rel, &sides, &letter, &sender);
                }
            }
        }
    }
}

impl Editor {
    fn active(&self) -> Option<usize> {
        self.nb.current_page().map(|c| c as usize)
    }

    /// the toplevel window (dialogs are transient for it — python get_root())
    fn window_widget(&self) -> Option<gtk::Window> {
        self.nb.root().and_downcast::<gtk::Window>()
    }

    /// editor.py:_error — modal CLOSE MessageDialog, transient for the window.
    fn error(&self, primary: &str, secondary: &str) {
        let Some(win) = self.window_widget() else { return };
        #[allow(deprecated)] // 4.10-deprecated upstream; editor.py uses it (S3 polish)
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
        self.pages.iter().position(|pg| pg.path.as_deref() == Some(path))
    }

    /// diff tabs key off "diff:<rel>" (python `_page_of`'s diff arm); the exact
    /// path match `page_of` stays file-tab-only
    fn page_of_key(&self, key: &str) -> Option<usize> {
        self.pages.iter().position(|pg| pg.diff_of.as_deref() == Some(key))
    }

    /// basename of a rel path (os.path.basename — the last '/' segment)
    fn rel_base(rel: &str) -> &str {
        rel.rsplit_once('/').map(|(_, b)| b).unwrap_or(rel)
    }

    // ---- lifecycle ------------------------------------------------------------

    /// editor.py:set_root — records the root, closes everything.
    fn set_root(&mut self, root: PathBuf) {
        self.root = Some(root);
        self.close_all();
    }

    /// editor.py:restore — set_root + open still-existing tabs + clamp by
    /// min(tab, n-1) (a saved index past the end re-clamps) + refresh.
    fn restore(&mut self, root: &Path, tabs: Vec<String>, active: i64, sender: &ComponentSender<Self>) {
        self.set_root(root.to_path_buf());
        for rel in tabs {
            let p = root.join(rel);
            if p.is_file() {
                // stale persisted tabs vanish silently
                self.open_file(&p, sender);
            }
        }
        let n = self.nb.n_pages() as i64;
        // GTK ignores a negative set_current_page; u32::try_from(None-maps it)
        self.nb
            .set_current_page(u32::try_from(active.min((n - 1).max(0))).ok());
        self.refresh_restamps(sender);
        let _ = sender.output(EditorOutput::StateChanged(self.snapshot()));
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

    // ---- open -----------------------------------------------------------------

    fn open_file(&mut self, path: &Path, sender: &ComponentSender<Self>) {
        if let Some(k) = self.page_of(path) {
            self.nb.set_current_page(Some(k as u32));
            let _ = sender.output(EditorOutput::StateChanged(self.snapshot()));
            return;
        }
        let raw = match std::fs::read(path) {
            Ok(bytes) => Some(bytes),
            // vanished since the tree rendered it → tombstone tab, never a popup
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                self.error(
                    "Couldn't open file",
                    &format!("{}: {e}", basename(path)),
                );
                return;
            }
        };
        // refusing at open is the only safe point: a lossy buffer + Ctrl+S
        // would rewrite the file with truncated/replacement garbage
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
        // GtkSource.Buffer(text=…) is born modified — an open must not show
        // the dirty dot
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
            .pixels_above_lines(2) // line pitch ~20 @ 13px mono (VS Code density)
            .left_margin(12)       // code column inset after the line-number gutter
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
            .ellipsize(pango::EllipsizeMode::Middle) // a deep path must not widen the page min
            .css_classes(["alpaca-breadcrumb"])
            .build();
        page_box.append(&crumb);
        // ref kept: reload restores its scroll
        let sw = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .child(&view)
            .build();
        page_box.append(&sw);

        let fs = if raw.is_some() { FileState::Live } else { FileState::Deleted };
        let load_mtime = if raw.is_some() { mtime_ns(path) } else { None };

        // tab head: dirty ● FIRST, badge slot, name, close
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
        // tabname floor: GTK 4.22 pins scrollable-notebook tabs at their
        // minimum even with space to spare — bare ellipsize floors the text at
        // '…' and GTK4 GtkLabel has no min-width-chars (GTK3-only).
        name.set_size_request(72, -1);
        let close = gtk::Button::new();
        if let Some(vec) = vector::icon("x-dim.svg") {
            close.set_child(Some(&gtk::Image::from_paintable(Some(&vec))));
        } else {
            close.set_child(Some(
                &gtk::Image::builder().icon_name("window-close-symbolic").build(),
            ));
        }
        close.set_css_classes(&["alpaca-close"]);
        let head = gtk::Box::builder()
            .spacing(4)
            .css_classes(["alpaca-tab"])
            .valign(gtk::Align::Center)
            .build();
        head.append(&dot); // must stay first
        // per-file type badge; restamped by refresh
        let slot = gtk::Box::builder().spacing(4).valign(gtk::Align::Center).build();
        head.append(&slot);
        head.append(&name);
        head.append(&close);
        if let Some(want) = badges::pixbuf_for(&base, false) {
            slot.append(&gtk::Image::from_pixbuf(Some(&want)));
        }
        let index = self.nb.append_page(&page_box, Some(&head));
        self.nb.set_current_page(Some(index as u32)); // append does not switch; a new tab must take focus
        // resolve the (possibly shifted) index at click time via the page widget
        let nb2 = self.nb.clone();
        let pg_widget = page_box.clone();
        let snd = sender.clone();
        close.connect_clicked(move |_| {
            // page Some only while the page exists
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
        self.refresh_restamps(sender); // switch-page alone misses opens that don't change pages
        let _ = sender.output(EditorOutput::StateChanged(self.snapshot()));
    }

    // ---- tab state / dots -----------------------------------------------------

    fn paint_dot(&self, idx: usize) {
        let pg = &self.pages[idx];
        // red while locally modified, amber while ALSO changed on disk
        pg.dot.set_visible(pg.buf.is_modified() || pg.conflict);
        let mut classes = vec!["alpaca-dirty".to_owned()];
        if pg.conflict {
            classes.push("conflict".to_owned()); // amber
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
        // every tab carries its file's type badge (user overrode the mockup's
        // hash-for-inactive rule: a selected tab must not erase the others')
        for pg in &self.pages {
            if pg.diff_of.is_some() {
                continue; // diff tab: badge_slot already holds its letter chip
            }
            while let Some(c) = pg.badge_slot.first_child() {
                pg.badge_slot.remove(&c);
            }
            if let Some(path) = &pg.path {
                if let Some(want) = badges::pixbuf_for(&basename(path), false) {
                    pg.badge_slot.append(&gtk::Image::from_pixbuf(Some(&want)));
                }
            }
        }
        let _ = sender.output(EditorOutput::StateChanged(self.snapshot()));
    }

    // ---- close ----------------------------------------------------------------

    fn close_index(&mut self, index: usize, force: bool, sender: &ComponentSender<Self>) {
        if index >= self.pages.len() {
            return;
        }
        if self.pages[index].buf.is_modified() && !force {
            // dirty → "Discard changes to X?" CANCEL/Discard; the Discard
            // click re-enters forced. The index is resolved at response time
            // via the page widget — tabs may shift while the dialog is open
            // (python captures a possibly-stale one; same UI outcome).
            let Some(win) = self.window_widget() else { return };
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
                dd.destroy(); // python always destroys in the response handler
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
        self.refresh_restamps(sender); // closing the active tab flips tabs
    }

    // ---- save -----------------------------------------------------------------

    fn write_page(&mut self, idx: usize) -> Result<(), std::io::Error> {
        let path = self.pages[idx].path.clone().expect("write_page: file page");
        let text = self.pages[idx]
            .buf
            .text(&self.pages[idx].buf.start_iter(), &self.pages[idx].buf.end_iter(), false);
        std::fs::write(&path, text.as_str())?;
        // conflict cleared BEFORE set_modified(false) — the fired
        // modified-changed repaints the dot (order matters)
        self.pages[idx].conflict = false;
        self.pages[idx].buf.set_modified(false);
        if let Some(m) = mtime_ns(&path) {
            self.pages[idx].load_mtime = Some(m); // latch the save — the settle must not reload what we wrote
        }
        Ok(())
    }

    fn save_active(&mut self, _sender: &ComponentSender<Self>) {
        let Some(idx) = self.active() else { return };
        if self.pages[idx].path.is_none() {
            return; // diff page (S2): Ctrl+S is a no-op
        }
        if !self.pages[idx].buf.is_modified() {
            return; // clean Ctrl+S never resurrects a tombstone note
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

    // ---- diff pages (S2) ------------------------------------------------------

    /// python `_diff_side` — one side: fresh GtkSource buffer (scheme + lang),
    /// bg tint over the marked lines, header fg, `set_modified` AFTER the tags
    /// ("diff tabs never dirty"), non-editable unnumbered mono view.
    fn diff_side(
        &self,
        lines: &[String],
        idxs: &BTreeSet<usize>,
        hdr: &BTreeSet<usize>,
        hexcol: &str,
        lang: Option<&sourceview5::Language>,
    ) -> (sourceview5::View, sourceview5::Buffer) {
        let buf = sourceview5::Buffer::builder().text(&lines.join("\n")).build();
        if let Some(scheme) = &self.scheme {
            buf.set_style_scheme(Some(scheme));
        }
        if let Some(lang) = lang {
            buf.set_language(Some(lang));
        }
        // editor.py:_rgba — a bad hex stays transparent (fresh Gdk.RGBA())
        let tint = rgba(hexcol);
        let tag = gtk::TextTag::builder().background_rgba(&tint).build();
        let fg = gtk::TextTag::builder()
            .foreground_rgba(&rgba(DIFF_HDR_FG))
            .build();
        buf.tag_table().add(&tag);
        buf.tag_table().add(&fg);
        // python's `i < 0` clamp — parse never stores negatives, so the usize
        // compare kills a wrapped usize::MAX too (untracked full-del rows)
        for &i in idxs.iter() {
            if i >= buf.line_count() as usize || i == usize::MAX {
                continue;
            }
            if let Some(a) = buf.iter_at_line(i as i32) {
                let mut b = a.clone();
                b.forward_to_line_end();
                buf.apply_tag(&tag, &a, &b);
            }
        }
        for &i in hdr.iter() {
            if i >= buf.line_count() as usize || i == usize::MAX {
                continue;
            }
            if let Some(a) = buf.iter_at_line(i as i32) {
                let mut b = a.clone();
                b.forward_to_line_end();
                buf.apply_tag(&fg, &a, &b);
            }
        }
        // born modified (invariant); diff tabs never dirty
        buf.set_modified(false);
        let view = sourceview5::View::builder()
            .buffer(&buf)
            .show_line_numbers(false) // padded sides would falsify numbers
            .editable(false)
            .wrap_mode(gtk::WrapMode::None)
            .pixels_above_lines(2)
            .left_margin(12)
            .css_classes(["alpaca-mono"])
            .build();
        (view, buf)
    }

    /// python editor.py:open_diff — a CHANGES row's side-by-side page: left
    /// (del, red tint) / right (add, green tint), linked scroll, letter chip
    /// in the tab head ("diff tabs never dirty" → no dot). Reuses the file
    /// page's widget fields for the LEFT side.
    fn open_diff(&mut self, rel: &str, sides: &Sides, letter: &str, sender: &ComponentSender<Self>) {
        let key = format!("diff:{rel}");
        if let Some(idx) = self.page_of_key(&key) {
            self.nb.set_current_page(Some(idx as u32));
            return;
        }
        let base = Self::rel_base(rel);
        let lang = self.lm.guess_language(Some(base), None);
        let (v_l, b_l) = self.diff_side(&sides.old, &sides.del, &sides.hdr, DIFF_DEL_BG, lang.as_ref());
        let (v_r, _b_r) = self.diff_side(&sides.new, &sides.add, &sides.hdr, DIFF_ADD_BG, lang.as_ref());

        let sw_l = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .child(&v_l)
            .build();
        let sw_r = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .child(&v_r)
            .build();
        let sep = gtk::Box::builder().css_classes(["alpaca-diffsep"]).build();
        let pair = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .build();
        pair.append(&sw_l);
        pair.append(&sep);
        pair.append(&sw_r);

        let page = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        let crumb = gtk::Label::builder()
            .label(rel.split('/').collect::<Vec<_>>().join("  ›  "))
            .xalign(0.0)
            .ellipsize(pango::EllipsizeMode::Middle) // a deep path must not widen the page min
            .css_classes(["alpaca-breadcrumb"])
            .build();
        page.append(&crumb);
        page.append(&pair);

        // tab head: letter chip slot FIRST, name, close — and no dirty dot.
        // python builds a dot anyway but never appends it (diff tabs never
        // dirty); an orphan for paint_dot parity
        let slot = gtk::Box::builder().spacing(4).valign(gtk::Align::Center).build();
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
                &gtk::Image::builder().icon_name("window-close-symbolic").build(),
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
        self.nb.set_current_page(Some(index as u32)); // append does not switch; a new tab must take focus
        // resolve the (possibly shifted) index at click time via the page widget
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
            linked_l: Rc::new(RefCell::new(None)),
            linked_r: Rc::new(RefCell::new(None)),
        };
        // wire takes two VIEWS — the dst is captured at wire time
        wire_diff(&v_l, &v_r, &diff.linked_l);
        wire_diff(&v_r, &v_l, &diff.linked_r);

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

    /// python editor.py:refresh_diff — live renewal of an open diff page:
    /// scroll fracs FIRST (the re-children resets both views), both sides
    /// rebuilt fresh, rewired, scroll restored, letter chip rebuilt on change.
    fn refresh_diff(&mut self, rel: &str, sides: &Sides, letter: &str, sender: &ComponentSender<Self>) {
        let key = format!("diff:{rel}");
        let Some(idx) = self.page_of_key(&key) else { return };
        let (frac_l, frac_r) = {
            let dw = self.pages[idx].diff.as_ref().unwrap(); // diff pages always carry theirs
            (frac_of(&dw.sw_l), frac_of(&dw.sw_r))
        };
        let lang = self.lm.guess_language(Some(Self::rel_base(rel)), None);
        let (v_l, _b_l) = self.diff_side(&sides.old, &sides.del, &sides.hdr, DIFF_DEL_BG, lang.as_ref());
        let (v_r, _b_r) = self.diff_side(&sides.new, &sides.add, &sides.hdr, DIFF_ADD_BG, lang.as_ref());
        {
            let dw = self.pages[idx].diff.as_ref().unwrap();
            dw.sw_l.set_child(Some(&v_l));
            dw.sw_r.set_child(Some(&v_r));
            // wire takes two VIEWS — the dst is captured at wire time
            wire_diff(&v_l, &v_r, &dw.linked_l);
            wire_diff(&v_r, &v_l, &dw.linked_r);
            restore_frac(&dw.sw_l, frac_l);
            restore_frac(&dw.sw_r, frac_r);
        }
        // python refresh_diff touches no page refs (its diff pages carry no
        // buf attr at all) and never calls _refresh_tab_state
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

    // ---- clip / edit ops ------------------------------------------------------

    /// Edit-menu op target (python window.py:_edit_op, review fix-3):
    /// buffer pages → the page's own view; diff pages → the CURRENTLY FOCUSED
    /// text view (either side panel), so Cut/Copy/Paste/Select-All act on the
    /// side the user actually has focus in; anything not focusable cleanly
    /// no-ops the activate_action. diff pages carry no editable buffer — the
    /// stored left view (PageState.view) is NOT the right target there.
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
        if let Some(idx) = self.active() {
            if let Some(w) = self.op_target(idx) {
                let _ = w.activate_action(op, None);
            }
        }
    }

    // ---- live file changes: no popups, disk is the truth ----------------------

    fn cancel_watch(&mut self, path: &Path) {
        if let Some(mon) = self.watchers.remove(path) {
            mon.cancel();
        }
    }

    /// editor.py:_watch — one FileMonitor per open path.
    fn arm_watch(&mut self, path: &Path, sender: &ComponentSender<Self>) {
        self.cancel_watch(path);
        let file = gio::File::for_path(path);
        // an absent path may have no watchable inode: tombstone stays until reopened
        let mon = match file.monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE) {
            Ok(m) => m,
            Err(_) => return,
        };
        let snd = sender.clone();
        let p = path.to_path_buf();
        mon.connect_changed(move |_, _, _, _| {
            // No popups (live spec). Every event — delete, atomic replace
            // (agents save via temp+rename), re-create — funnels into ONE
            // debounced disk check.
            snd.input(EditorMsg::FsEvent(p.clone()));
        });
        self.watchers.insert(path.to_path_buf(), mon);
    }

    fn arm_settle(&self, path: PathBuf, sender: &ComponentSender<Self>) {
        // python resets the debounce (pop + source_remove), but SourceId::remove
        // PANICS on an already-fired id (measured: one touch+delete race kills the
        // app). fs_settled is idempotent, so stale settles are harmless — arm
        // without cancelling.
        let snd = sender.clone();
        glib::timeout_add(std::time::Duration::from_millis(120), move || {
            snd.input(EditorMsg::FsSettled(path.clone()));
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
            return; // our own open/Ctrl+S echo
        }
        if self.pages[k].buf.is_modified() {
            if self.pages[k].fs != FileState::Live {
                // dirty buffer on a placeholder tab: the note dies, text lives
                self.pages[k].fs = FileState::Live;
                self.paint_tab_deleted(k, false);
            }
            self.pages[k].conflict = true;
            self.paint_dot(k);
            return;
        }
        self.reload(k, mtime, sender); // clean buffer follows the disk (also un-tombstones)
    }

    fn reload(&mut self, idx: usize, mtime: i64, sender: &ComponentSender<Self>) {
        let Some(path) = self.pages[idx].path.clone() else { return };
        let Ok(raw) = std::fs::read(&path) else {
            self.fs_tombstone(idx, sender);
            return;
        };
        let Some(text) = readable_text(&raw) else {
            self.fs_binary(idx, sender);
            return;
        };
        // cursor line/col + scroll value preserved across the seamless swap
        let (line, col, scroll) = {
            let pg = &self.pages[idx];
            let ins = pg.buf.get_insert();
            let it = pg.buf.iter_at_mark(&ins);
            (it.line(), it.line_offset(), pg.sw.vadjustment().value())
        };
        self.swap_text(idx, &text, sender);
        // place_cursor clamped to the new buffer's shape
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

    /// fresh buffer (style + language carried over); the view keeps its place
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
        nb.set_modified(false); // born modified (invariant) — a reload must not raise the dot
        let snd = sender.clone();
        nb.connect_modified_changed(move |_| snd.input(EditorMsg::StateTick));
        let pg = &mut self.pages[idx];
        pg.view.set_buffer(Some(&nb));
        pg.buf = nb;
    }

    fn fs_tombstone(&mut self, idx: usize, sender: &ComponentSender<Self>) {
        // file gone while open: italic, dimmed tab; a CLEAN buffer becomes the
        // tombstone note, a DIRTY one keeps the user's text (Ctrl+S recreates
        // the file). A later re-created file un-tombstones via reload.
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
        // disk content unreadable: the buffer could never round-trip it, so
        // freeze the view and note why. The unmodified note can never be saved
        // back over the binary file (dirty save guard); a later readable file
        // un-freezes via reload.
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

    /// muted italic whole-buffer tag (placeholder notes)
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
        // index, not a path; restore re-clamps via min(saved, n-1)
        let active_tab = self.nb.current_page().map(i64::from).unwrap_or(0);
        let has_dirty = self.pages.iter().any(|pg| pg.buf.is_modified());
        EditorSnapshot { open_tabs, active_tab, has_dirty }
    }

}

#[allow(deprecated)] // 4.10-deprecated upstream; editor.py uses it (S3 polish)
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

/// fracs at renewal — measured law: the ScrolledWindow swaps the view's
/// placeholder adjustment for its own at the page's first allocation, WITHOUT
/// any property notify (notify::vadjustment never fires on view or sw). All
/// linking therefore dedupes by adjustment identity (Adjustment PartialEq) and
/// retries on allocation notify.
///
/// Also python `_diff_resync`'s idle retry — cover a post-emit swap in the
/// same frame.

/// python `_rgba` — fresh Gdk.RGBA() (transparent) that parse() fills in
/// place; a bad hex stays transparent (an invisible tag, not a panic)
fn rgba(hexcol: &str) -> gtk::gdk::RGBA {
    let mut c = gtk::gdk::RGBA::new(0.0, 0.0, 0.0, 0.0);
    if let Ok(p) = gtk::gdk::RGBA::parse(hexcol) {
        c = p;
    }
    c
}

/// capture the scroll fraction BEFORE a re-child (python `_on_git_changed`'s
/// leading _scroll_frac reads)
fn frac_of(sw: &gtk::ScrolledWindow) -> f64 {
    let adj = sw.vadjustment(); // inherent getter: non-Option (only views are)
    let span = (adj.upper() - adj.page_size()).max(0.0);
    if span > 0.0 {
        (adj.value() / span).min(1.0)
    } else {
        0.0
    }
}

/// python `_scroll_frac` — reapply a captured fraction AFTER the rebuild;
/// fetch vadj at run time (unallocated adj reads 0-span → top, brief churn)
fn restore_frac(sw: &gtk::ScrolledWindow, frac: f64) {
    let s2 = sw.clone();
    glib::idle_add_local_once(move || {
        let adj = s2.vadjustment(); // inherent getter: non-Option
        let span = (adj.upper() - adj.page_size()).max(0.0);
        adj.set_value((frac * span).min(span));
    });
}

/// python `_diff_link` — drive `dst`'s scroll from `v`'s. True = a NEW link
/// was made (equal-value set_value fires no changed signal → no loop, so the
/// destination never needs a guard).
fn diff_link(
    v: &sourceview5::View,
    dst: &sourceview5::View,
    linked: &Rc<RefCell<Option<gtk::Adjustment>>>,
) -> bool {
    // a pre-allocation View owns NO adjustment at all on this build (vadjustment
    // is Option) — return unlinked, the allocation-notify retry covers it
    let Some(va) = v.vadjustment() else { return false };
    if *linked.borrow() == Some(va.clone()) {
        return false; // already on the live adj
    }
    *linked.borrow_mut() = Some(va.clone());
    let dst2 = dst.clone();
    va.connect_value_changed(move |a| {
        // dst's ScrollableExt getter is Option (pre-allocation None — the
        // placeholder law); equal-value set_value fires nothing anyway
        if let Some(dadj) = dst2.vadjustment() {
            dadj.set_value(a.value());
        }
    });
    true
}

/// python `_wire_diff_scroll` — link once, then retry the link on every
/// allocation change (deduped — no re-link, no signal loop) + one idle retry
/// covering a post-emit swap. Strong refs, python-exact (`lambda a, o=dst`):
/// v → handler → dst is a DAG (no cycle) — a closed page frees the chain.
fn wire_diff(
    view: &sourceview5::View,
    dst: &sourceview5::View,
    linked: &Rc<RefCell<Option<gtk::Adjustment>>>,
) {
    diff_link(view, dst, linked);
    let v2 = view.clone();
    let dst2 = dst.clone();
    let l2 = linked.clone();
    view.connect_notify_local(Some("allocation"), move |_, _| {
        if diff_link(&v2, &dst2, &l2) {
            let v3 = v2.clone();
            let d3 = dst2.clone();
            let l3 = l2.clone();
            glib::idle_add_local_once(move || {
                diff_link(&v3, &d3, &l3);
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::readable_text;

    #[test]
    fn readable_text_rules() {
        assert_eq!(readable_text(b"hello\nworld").as_deref(), Some("hello\nworld"));
        assert!(readable_text(b"caf\xc3\xa9").is_some()); // valid UTF-8 non-ASCII opens
        assert_eq!(readable_text(b"caf\xe9"), None); // invalid UTF-8
        assert_eq!(readable_text(b"a\0b"), None); // NUL truncation trap
        assert_eq!(readable_text(b""), Some(String::new())); // empty file is fine
    }
}